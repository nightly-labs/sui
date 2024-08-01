// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_core_types::language_storage::StructTag;
use odin::sui_ws::{ObjectChangeUpdate, ObjectUpdateStatus, Received, Sent};
use std::collections::{BTreeMap, HashMap};
use sui_json_rpc_types::ObjectChange;
use sui_types::base_types::{ObjectID, ObjectRef, SequenceNumber, SuiAddress};
use sui_types::effects::ObjectRemoveKind;
use sui_types::object::{Object, Owner};
use sui_types::storage::WriteKind;
use tracing::warn;

use crate::ObjectProvider;

pub async fn get_object_changes<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    sender: SuiAddress,
    modified_at_versions: Vec<(ObjectID, SequenceNumber)>,
    all_changed_objects: Vec<(ObjectRef, Owner, WriteKind)>,
    all_removed_objects: Vec<(ObjectRef, ObjectRemoveKind)>,
) -> Result<Vec<ObjectChange>, E> {
    let mut object_changes = vec![];

    let modify_at_version = modified_at_versions.into_iter().collect::<BTreeMap<_, _>>();

    for ((object_id, version, digest), owner, kind) in all_changed_objects {
        let o = object_provider.get_object(&object_id, &version).await?;
        if let Some(type_) = o.type_() {
            let object_type = type_.clone().into();

            match kind {
                WriteKind::Mutate => object_changes.push(ObjectChange::Mutated {
                    sender,
                    owner,
                    object_type,
                    object_id,
                    version,
                    // modify_at_version should always be available for mutated object
                    previous_version: modify_at_version
                        .get(&object_id)
                        .cloned()
                        .unwrap_or_default(),
                    digest,
                }),
                WriteKind::Create => object_changes.push(ObjectChange::Created {
                    sender,
                    owner,
                    object_type,
                    object_id,
                    version,
                    digest,
                }),
                _ => {}
            }
        } else if let Some(p) = o.data.try_as_package() {
            if kind == WriteKind::Create {
                object_changes.push(ObjectChange::Published {
                    package_id: p.id(),
                    version: p.version(),
                    digest,
                    modules: p.serialized_module_map().keys().cloned().collect(),
                })
            }
        };
    }

    for ((id, version, _), kind) in all_removed_objects {
        let o = object_provider
            .find_object_lt_or_eq_version(&id, &version)
            .await?;
        if let Some(o) = o {
            if let Some(type_) = o.type_() {
                let object_type = type_.clone().into();
                match kind {
                    ObjectRemoveKind::Delete => object_changes.push(ObjectChange::Deleted {
                        sender,
                        object_type,
                        object_id: id,
                        version,
                    }),
                    ObjectRemoveKind::Wrap => object_changes.push(ObjectChange::Wrapped {
                        sender,
                        object_type,
                        object_id: id,
                        version,
                    }),
                }
            }
        };
    }

    Ok(object_changes)
}

pub async fn custom_get_object_changes<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    sender: SuiAddress,
    modified_at_versions: Vec<(ObjectID, SequenceNumber)>,
    all_changed_objects: Vec<(ObjectRef, Owner, WriteKind)>,
    all_removed_objects: Vec<(ObjectRef, ObjectRemoveKind)>,
    input_objects: &Vec<Object>,
    output_objects: &Vec<Object>,
) -> Result<(Vec<ObjectChange>, Vec<(Option<String>, ObjectChangeUpdate)>), E> {
    let mut object_changes = vec![];
    let mut custom_object_changes: Vec<(Option<String>, ObjectChangeUpdate)> = vec![];

    // Input objects ownership map
    let input_ownership_map = input_objects
        .iter()
        .map(|o| {
            (
                o.id().to_string(),
                match o.owner.get_owner_address() {
                    Ok(owner) => Some(owner.to_string()),
                    Err(_) => None,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    // Output objects ownership map is needed to check if object ownership changed
    let output_ownership_map = output_objects
        .iter()
        .map(|o| {
            (
                o.id().to_string(),
                match o.owner.get_owner_address() {
                    Ok(owner) => Some(owner.to_string()),
                    Err(_) => None,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let modify_at_version = modified_at_versions.into_iter().collect::<BTreeMap<_, _>>();

    for ((object_id, version, digest), owner, kind) in all_changed_objects {
        let o = object_provider.get_object(&object_id, &version).await?;
        if let Some(type_) = o.type_() {
            let object_type: StructTag = type_.clone().into();

            let address_owner = match o.owner.get_owner_address() {
                Ok(owner) => Some(owner.to_string()),
                Err(_) => None,
            };

            let data = match o.data.try_as_move() {
                Some(data) => Some(data.clone().into_contents()),
                None => None,
            };

            match kind {
                WriteKind::Mutate => {
                    object_changes.push(ObjectChange::Mutated {
                        sender,
                        owner,
                        object_type: object_type.clone(),
                        object_id,
                        version,
                        // modify_at_version should always be available for mutated object
                        previous_version: modify_at_version
                            .get(&object_id)
                            .cloned()
                            .unwrap_or_default(),
                        digest,
                    });

                    // Check if ownership change occurred, object should have existed in transaction input
                    let object_owner = match input_ownership_map.get(&object_id.to_string()) {
                        Some(old_owner) => old_owner.clone(),
                        None => {
                            // Should never happen
                            warn!(
                                "Object ownership change occurred but object not found in input objects, object_id: {}", object_id
                            );
                            continue;
                        }
                    };

                    // Check change
                    match (object_owner, address_owner) {
                        (Some(old_owner), Some(new_owner)) => {
                            // Check if owner changed
                            if old_owner != new_owner {
                                // Receiver
                                custom_object_changes.push((
                                    Some(new_owner.clone()),
                                    ObjectChangeUpdate {
                                        object_id: object_id.to_string(),
                                        object_type_tag: Some(
                                            object_type.to_canonical_string(true),
                                        ),
                                        object_version: Some(version.into()),
                                        object_bcs: data,
                                        object_metadata: None,
                                        status: ObjectUpdateStatus::Received(Received {
                                            sender_address: old_owner.to_string(),
                                            receiver_address: new_owner,
                                        }),
                                    },
                                ));
                            }
                        }
                        (Some(_), None) | (None, None) => {
                            custom_object_changes.push((
                                None,
                                ObjectChangeUpdate {
                                    object_id: object_id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: data,
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Mutated,
                                },
                            ));
                        }
                        (None, Some(new_owner)) => {
                            // Object did not have owner before, now has owner
                            custom_object_changes.push((
                                Some(new_owner),
                                ObjectChangeUpdate {
                                    object_id: object_id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: data,
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Mutated,
                                },
                            ));
                        }
                    }
                }
                WriteKind::Create => {
                    object_changes.push(ObjectChange::Created {
                        sender,
                        owner,
                        object_type: object_type.clone(),
                        object_id,
                        version,
                        digest,
                    });

                    // Check if object had previous owner
                    let object_existed = input_ownership_map.get(&object_id.to_string());

                    // 1. (Some(), Some()) Object existed and still has owner
                    // 2. (Some(), None) Object existed but no longer has owner
                    // 3. (None, Some()) Object did not exist and now does and has an owner
                    // 4. (None, None) Object did not exist and but now it does and it does not have an owner
                    match (object_existed, address_owner) {
                        // (Option<String>, Option<String>)
                        (Some(old_owner), Some(new_owner)) => {
                            // Check if object had owner before and now has a different owner
                            match old_owner {
                                Some(old_owner) => {
                                    // object existed and had owner before, check if owner changed
                                    if old_owner != &new_owner {
                                        // Receiver
                                        custom_object_changes.push((
                                            Some(new_owner.clone()),
                                            ObjectChangeUpdate {
                                                object_id: object_id.to_string(),
                                                object_type_tag: Some(
                                                    object_type.to_canonical_string(true),
                                                ),
                                                object_version: Some(version.into()),
                                                object_bcs: data,
                                                object_metadata: None,
                                                status: ObjectUpdateStatus::Received(Received {
                                                    sender_address: old_owner.to_string(),
                                                    receiver_address: new_owner,
                                                }),
                                            },
                                        ));
                                    }
                                }
                                None => {
                                    // Object did not have owner before, now has owner
                                    custom_object_changes.push((
                                        Some(new_owner),
                                        ObjectChangeUpdate {
                                            object_id: object_id.to_string(),
                                            object_type_tag: Some(
                                                object_type.to_canonical_string(true),
                                            ),
                                            object_version: Some(version.into()),
                                            object_bcs: data,
                                            object_metadata: None,
                                            status: ObjectUpdateStatus::Mutated,
                                        },
                                    ));
                                }
                            }
                        }
                        (Some(_), None) | (None, None) => {
                            custom_object_changes.push((
                                None,
                                ObjectChangeUpdate {
                                    object_id: object_id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: data,
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Created,
                                },
                            ));
                        }
                        // Object did not exist before, now it does and has an owner
                        (None, Some(new_owner)) => {
                            custom_object_changes.push((
                                Some(new_owner),
                                ObjectChangeUpdate {
                                    object_id: object_id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: data,
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Created,
                                },
                            ));
                        }
                    }
                }
                _ => {}
            }
        } else if let Some(p) = o.data.try_as_package() {
            if kind == WriteKind::Create {
                object_changes.push(ObjectChange::Published {
                    package_id: p.id(),
                    version: p.version(),
                    digest,
                    modules: p.serialized_module_map().keys().cloned().collect(),
                })
            }
        };
    }

    for ((id, version, _), kind) in all_removed_objects {
        let o = object_provider
            .find_object_lt_or_eq_version(&id, &version)
            .await?;
        if let Some(o) = o {
            if let Some(type_) = o.type_() {
                let object_type: StructTag = type_.clone().into();
                match kind {
                    ObjectRemoveKind::Delete => object_changes.push(ObjectChange::Deleted {
                        sender,
                        object_type: object_type.clone(),
                        object_id: id,
                        version,
                    }),
                    ObjectRemoveKind::Wrap => object_changes.push(ObjectChange::Wrapped {
                        sender,
                        object_type: object_type.clone(),
                        object_id: id,
                        version,
                    }),
                }

                let data = match o.data.try_as_move() {
                    Some(data) => data.clone().into_contents(),
                    None => continue,
                };

                let address_owner = match o.owner.get_owner_address() {
                    Ok(owner) => Some(owner.to_string()),
                    Err(_) => None,
                };

                // Check if object has changed ownership
                match output_ownership_map.get(&id.to_string()) {
                    // object still exists aka has changed ownership
                    Some(new_owner) => {
                        // just in case get old owner from out map
                        let old_owner = match input_ownership_map.get(&id.to_string()) {
                            Some(old_owner) => old_owner,
                            None => {
                                // Should never happen
                                warn!(
                                    "Object ownership change occurred but object not found in input objects, object_id: {}", id
                                );
                                continue;
                            }
                        };

                        if let Some(new_owner) = new_owner {
                            // Sender
                            custom_object_changes.push((
                                old_owner.clone(),
                                ObjectChangeUpdate {
                                    object_id: id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: Some(data),
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Sent(Sent {
                                        sender_address: new_owner.clone(),
                                        receiver_address: new_owner.clone(),
                                    }),
                                },
                            ));
                        } else {
                            // Object was transferred, exists but no longer has owner
                            custom_object_changes.push((
                                old_owner.clone(),
                                ObjectChangeUpdate {
                                    object_id: id.to_string(),
                                    object_type_tag: Some(object_type.to_canonical_string(true)),
                                    object_version: Some(version.into()),
                                    object_bcs: Some(data),
                                    object_metadata: None,
                                    status: ObjectUpdateStatus::Deleted,
                                },
                            ));
                        }
                    }
                    None => {
                        // Object was simply deleted
                        custom_object_changes.push((
                            address_owner,
                            ObjectChangeUpdate {
                                object_id: id.to_string(),
                                object_type_tag: Some(object_type.to_canonical_string(true)),
                                object_version: Some(version.into()),
                                object_bcs: Some(data),
                                object_metadata: None,
                                status: ObjectUpdateStatus::Deleted,
                            },
                        ));
                    }
                }
            }
        };
    }

    Ok((object_changes, custom_object_changes))
}
