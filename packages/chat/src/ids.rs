//! Deterministic Chat entity and relation identifiers derived from authoritative inputs.

use rintawa_sdk::world::{CommandId, EntityId, RelationId};
use sha2::{Digest, Sha256};

pub(crate) fn command_entity_id(command_id: CommandId) -> EntityId {
    EntityId::from_bytes(command_id.into_bytes())
}

pub(crate) fn derived_entity_id(command_id: CommandId, role: &str) -> EntityId {
    let command_bytes = command_id.into_bytes();
    EntityId::from_bytes(derive_uuid_bytes(
        b"rintawa.chat.entity.v1",
        &[&command_bytes, role.as_bytes()],
    ))
}

pub(crate) fn relation_id(tag: &str, entities: &[EntityId]) -> RelationId {
    let entity_bytes = entities
        .iter()
        .map(|entity| entity.into_bytes())
        .collect::<Vec<_>>();
    let chunks = entity_bytes
        .iter()
        .map(<[u8; 16]>::as_slice)
        .chain(std::iter::once(tag.as_bytes()))
        .collect::<Vec<_>>();
    RelationId::from_bytes(derive_uuid_bytes(b"rintawa.chat.relation.v1", &chunks))
}

fn derive_uuid_bytes(domain: &[u8], chunks: &[&[u8]]) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for chunk in chunks {
        hasher.update((chunk.len() as u64).to_be_bytes());
        hasher.update(chunk);
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_derive_stable_distinct_chat_ids() {
        let command = CommandId::new();
        let entity = command_entity_id(command);
        assert_eq!(entity.into_bytes(), command.into_bytes());

        let revision = derived_entity_id(command, "revision");
        assert_ne!(revision, entity);
        assert_eq!(revision, derived_entity_id(command, "revision"));
        assert_ne!(
            relation_id("message-author", &[entity, revision]),
            relation_id("message-parent", &[entity, revision])
        );
    }
}
