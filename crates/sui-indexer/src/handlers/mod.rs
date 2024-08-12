// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{
    models::display::StoredDisplay,
    types::{
        CustomIndexedTransaction, IndexedCheckpoint, IndexedDeletedObject, IndexedEpochInfo,
        IndexedEvent, IndexedObject, IndexedPackage, IndexedTransaction, TxIndex,
    },
};

pub mod checkpoint_handler;
pub mod committer;
pub mod objects_snapshot_processor;
pub mod pruner;
pub mod tx_processor;

#[derive(Debug)]
pub struct CheckpointDataToCommit {
    pub checkpoint: IndexedCheckpoint,
    pub transactions: Vec<IndexedTransaction>,
    pub events: Vec<IndexedEvent>,
    pub tx_indices: Vec<TxIndex>,
    pub display_updates: BTreeMap<String, StoredDisplay>,
    pub object_changes: TransactionObjectChangesToCommit,
    pub object_history_changes: TransactionObjectChangesToCommit,
    pub packages: Vec<IndexedPackage>,
    pub epoch: Option<EpochToCommit>,
}

#[derive(Debug)]
pub struct CustomCheckpointDataToCommit {
    pub checkpoint: IndexedCheckpoint,
    pub transactions: Vec<CustomIndexedTransaction>,
    pub events: Vec<IndexedEvent>,
    pub tx_indices: Vec<TxIndex>,
    pub display_updates: BTreeMap<String, StoredDisplay>,
    pub object_changes: TransactionObjectChangesToCommit,
    pub object_history_changes: TransactionObjectChangesToCommit,
    pub packages: Vec<IndexedPackage>,
    pub epoch: Option<EpochToCommit>,
}

impl From<CustomCheckpointDataToCommit> for CheckpointDataToCommit {
    fn from(data: CustomCheckpointDataToCommit) -> Self {
        Self {
            checkpoint: data.checkpoint,
            transactions: data.transactions.into_iter().map(Into::into).collect(),
            events: data.events,
            tx_indices: data.tx_indices,
            display_updates: data.display_updates,
            object_changes: data.object_changes,
            object_history_changes: data.object_history_changes,
            packages: data.packages,
            epoch: data.epoch,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransactionObjectChangesToCommit {
    pub changed_objects: Vec<IndexedObject>,
    pub deleted_objects: Vec<IndexedDeletedObject>,
}

#[derive(Clone, Debug)]
pub struct EpochToCommit {
    pub last_epoch: Option<IndexedEpochInfo>,
    pub new_epoch: IndexedEpochInfo,
}
