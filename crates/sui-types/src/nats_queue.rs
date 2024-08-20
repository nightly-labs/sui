// We want to make sure we send msg about block in correct order
// This task is responsible for gathering all the messages and sending them in correct order
// We will use a priority queue to store the messages and send them in correct order

use odin::{structs::sui_notifications::SuiIndexerNotification, sui_ws::SuiWsApiMsg, Odin};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{
    mpsc::{channel, Receiver, Sender},
    Mutex,
};
use tracing::info;

pub type WsPayload = (u64, Vec<SuiWsApiMsg>);
pub type NotificationPayload = (u64, BTreeMap<u64, Vec<SuiIndexerNotification>>);

pub struct NatsQueueSender {
    pub init_checkpoint: u64,
    pub ws_sender: Arc<Sender<WsPayload>>,
    pub ws_receiver: Arc<Mutex<Receiver<WsPayload>>>,
    pub notifications_sender: Arc<Sender<NotificationPayload>>,
    pub notifications_receiver: Arc<Mutex<Receiver<NotificationPayload>>>,
    odin: Arc<Odin>,
}

pub fn nats_queue(odin: Arc<Odin>) -> NatsQueueSender {
    // Create sender and receiver for ws updates
    let (tx, rx) = channel::<WsPayload>(10_000);
    // Create sender and receiver for notifications
    let (tx_notifications, rx_notifications) = channel::<NotificationPayload>(10_000);

    NatsQueueSender {
        init_checkpoint: u64::MAX,
        ws_sender: Arc::new(tx),
        ws_receiver: Arc::new(Mutex::new(rx)),
        notifications_sender: Arc::new(tx_notifications),
        notifications_receiver: Arc::new(Mutex::new(rx_notifications)),
        odin,
    }
}

impl NatsQueueSender {
    pub async fn run(&mut self) {
        // Spawn task that will order the messages
        let odin = self.odin.clone();
        let receiver = self.ws_receiver.clone();
        let init_checkpoint = self.init_checkpoint;

        // Task for ws updates
        tokio::spawn(async move {
            let mut next_index: u64 = init_checkpoint; // MAX means we have not received any message yet

            let mut receiver_lock = receiver.lock().await;

            // Log init checkpoint
            info!("Nats queue init checkpoint: {}", init_checkpoint);

            //Cache if we get a message with a block number that is not in order
            let mut bmap_checkpoints: BTreeMap<u64, Vec<SuiWsApiMsg>> = BTreeMap::new();
            while let Some((checkpoint_seq_number, ws_updates)) = receiver_lock.recv().await {
                // Check if we have not received any message yet
                if next_index == u64::MAX {
                    next_index = checkpoint_seq_number;
                }
                // Check if correct order
                if checkpoint_seq_number == next_index {
                    // Send message

                    info!(
                        "Sending: {} ws updates with seq number {}",
                        ws_updates.len(),
                        checkpoint_seq_number
                    );
                    for ws_update in ws_updates.iter() {
                        odin.publish_sui_ws_update(&ws_update).await;
                    }

                    // Update next index
                    next_index = next_index + 1;
                    // Check if we have any cached messages
                    while let Some(next_checkpoint) = bmap_checkpoints.remove(&next_index) {
                        info!(
                            "Sending: {} cached ws updates with seq number {}",
                            next_checkpoint.len(),
                            next_index
                        );

                        for ws_update in next_checkpoint.iter() {
                            odin.publish_sui_ws_update(&ws_update).await;
                        }

                        // Update next index
                        next_index = next_index + 1;
                    }
                } else {
                    info!(
                        "Received checkpoint with seq number {} but expected {}",
                        checkpoint_seq_number, next_index
                    );
                    // Cache message
                    bmap_checkpoints
                        .entry(checkpoint_seq_number)
                        .or_insert(vec![])
                        .extend(ws_updates);
                }
            }
        });

        // Task for notifications
        let odin = self.odin.clone();
        let notifications_receiver = self.notifications_receiver.clone();
        tokio::spawn(async move {
            let mut next_index: u64 = init_checkpoint; // MAX means we have not received any message yet

            let mut receiver_lock = notifications_receiver.lock().await;

            // Log init checkpoint
            info!(
                "Nats notifications queue init checkpoint: {}",
                init_checkpoint
            );

            //Cache if we get a message with a block number that is not in order
            let mut bmap_checkpoints: BTreeMap<u64, BTreeMap<u64, Vec<SuiIndexerNotification>>> =
                BTreeMap::new();

            while let Some((checkpoint_seq_number, notifications)) = receiver_lock.recv().await {
                // Check if we have not received any message yet
                if next_index == u64::MAX {
                    next_index = checkpoint_seq_number
                }
                // Check if correct order
                if checkpoint_seq_number == next_index {
                    // Send message
                    info!(
                        "Sending: {} notifications with seq number {}",
                        notifications.len(),
                        next_index
                    );

                    // Iter over notifications and ordered by sequence number send them
                    for (_, notifications) in notifications.iter() {
                        odin.publish_sui_notifications(&notifications).await;
                    }

                    // Update next index
                    next_index = next_index + 1;
                    // Check if we have any cached messages
                    while let Some(next_checkpoint) = bmap_checkpoints.remove(&next_index) {
                        info!(
                            "Sending: {} cached notifications with seq number {}",
                            next_checkpoint.len(),
                            next_index
                        );
                        // Iter over notifications and ordered by sequence number send them
                        for (_, notifications) in notifications.iter() {
                            odin.publish_sui_notifications(&notifications).await;
                        }

                        // Update next index
                        next_index = next_index + 1;
                    }
                } else {
                    info!(
                        "Received checkpoint with seq number {} but expected {}",
                        checkpoint_seq_number, next_index
                    );
                    // Cache message
                    bmap_checkpoints
                        .entry(checkpoint_seq_number)
                        .or_insert(BTreeMap::new())
                        .extend(notifications);
                }
            }
        });
    }
}
