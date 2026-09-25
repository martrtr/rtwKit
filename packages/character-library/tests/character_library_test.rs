//! Integration tests for Character Library import, export, packaging, and instantiation planning.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::ZlibEncoder};
use rintawa_artifacts::{ArtifactDigest, RtwArchive, RtwLimits};
use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CHARACTER_TEMPLATE_ENTRY_PATH, CharacterTemplate,
    character_world_schemas, export_tavern_v2_json, import_tavern_v2, instantiate_character,
    pack_character_template_rtw, validate_character_template_descriptor,
};
use rintawa_sdk::{
    content::{ContentHandlerRequest, ContentHandlerResponse},
    world::SchemaKind,
};
use serde_json::{Value, json};
use std::io::Write;

fn card_json() -> Value {
    json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "vendor_top": { "keep": true },
        "data": {
            "name": "Alice",
            "description": "Portable description",
            "personality": "Curious",
            "scenario": "A train platform",
            "first_mes": "Hello from the platform.",
            "mes_example": "<START>\nAlice: Example",
            "creator_notes": "Creator note",
            "system_prompt": "Stay in character.",
            "post_history_instructions": "Prefer concise replies.",
            "alternate_greetings": ["Hi.", "Good evening."],
            "tags": ["test", "traveler"],
            "creator": "tester",
            "character_version": "7",
            "character_book": {
                "name": "Alice lore",
                "entries": [{"keys": ["platform"], "content": "A place."}]
            },
            "extensions": {
                "vendor/example": { "voice": "sample" },
                "unknown_scalar": 42
            },
            "future_field": { "must": "survive" }
        }
    })
}

#[test]
fn test_should_normalize_tavern_v2_and_preserve_compatibility_fields() -> anyhow::Result<()> {
    let source = serde_json::to_vec(&card_json())?;
    let mut template = import_tavern_v2(&source)?;

    assert_eq!(template.name, "Alice");
    assert_eq!(template.description, "Portable description");
    assert_eq!(template.personality, "Curious");
    assert_eq!(template.narration.scenario, "A train platform");
    assert_eq!(template.narration.system_prompt, "Stay in character.");
    assert_eq!(template.session.greeting, "Hello from the platform.");
    assert_eq!(
        template.session.alternate_greetings,
        ["Hi.", "Good evening."]
    );
    assert!(template.narration.character_book.is_some());

    template.name = String::from("Alice edited");
    template.session.greeting = String::from("Edited greeting");
    let exported: Value = serde_json::from_slice(&export_tavern_v2_json(&template)?)?;
    assert_eq!(exported["data"]["name"], "Alice edited");
    assert_eq!(exported["data"]["first_mes"], "Edited greeting");
    assert_eq!(exported["vendor_top"]["keep"], true);
    assert_eq!(exported["data"]["future_field"]["must"], "survive");
    assert_eq!(exported["data"]["extensions"]["unknown_scalar"], 42);
    assert_eq!(
        exported["data"]["extensions"]["vendor/example"]["voice"],
        "sample"
    );
    assert_eq!(exported["data"]["character_book"]["name"], "Alice lore");
    Ok(())
}

#[test]
fn test_should_import_base64_tavern_v2_from_png_text_chunk() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let encoded = BASE64.encode(&json);
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(encoded.as_bytes());

    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);

    let template = import_tavern_v2(&png)?;
    assert_eq!(template.name, "Alice");
    assert_eq!(template.session.alternate_greetings.len(), 2);
    Ok(())
}

#[test]
fn test_should_import_compressed_and_international_png_text_carriage() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let encoded = BASE64.encode(&json);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(encoded.as_bytes())?;
    let compressed_text = encoder.finish()?;
    let mut ztxt = b"chara\0\0".to_vec();
    ztxt.extend_from_slice(&compressed_text);
    let mut ztxt_png = png_header();
    push_png_chunk(&mut ztxt_png, b"zTXt", &ztxt);
    push_png_chunk(&mut ztxt_png, b"IEND", &[]);
    assert_eq!(import_tavern_v2(&ztxt_png)?.name, "Alice");

    let mut itxt = b"chara\0\0\0\0\0".to_vec();
    itxt.extend_from_slice(encoded.as_bytes());
    let mut itxt_png = png_header();
    push_png_chunk(&mut itxt_png, b"iTXt", &itxt);
    push_png_chunk(&mut itxt_png, b"IEND", &[]);
    assert_eq!(import_tavern_v2(&itxt_png)?.name, "Alice");
    Ok(())
}

#[test]
fn test_should_import_legacy_base64_zlib_json_payload() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json)?;
    let encoded = BASE64.encode(encoder.finish()?);
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(encoded.as_bytes());
    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);
    assert_eq!(import_tavern_v2(&png)?.name, "Alice");
    Ok(())
}

#[test]
fn test_should_reject_png_with_corrupt_chunk_crc() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(BASE64.encode(json).as_bytes());
    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    let last = png.len() - 1;
    png[last] ^= 0xff;

    assert!(import_tavern_v2(&png).is_err());
    Ok(())
}

#[test]
fn test_should_validate_remaining_png_chunks_after_card_metadata() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(BASE64.encode(json).as_bytes());
    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    let corrupt_start = png.len();
    push_png_chunk(&mut png, b"tEXt", b"note\0after-card");
    let corrupt_crc = png.len() - 1;
    png[corrupt_crc] ^= 0xff;
    push_png_chunk(&mut png, b"IEND", &[]);

    assert!(corrupt_start < corrupt_crc);
    assert!(import_tavern_v2(&png).is_err());
    Ok(())
}

#[test]
fn test_should_pack_character_template_as_generic_rtw_content() -> anyhow::Result<()> {
    let template = import_tavern_v2(&serde_json::to_vec(&card_json())?)?;
    let root = tempfile::TempDir::new()?;
    let output = root.path().join("alice.rtw");
    pack_character_template_rtw(&template, &output)?;

    let mut archive = RtwArchive::open(&output, RtwLimits::default())?;
    assert_eq!(
        archive.manifest().content.to_string(),
        CHARACTER_TEMPLATE_CONTENT_V1
    );
    assert_eq!(
        archive.manifest().entry.as_str(),
        CHARACTER_TEMPLATE_ENTRY_PATH
    );
    let entry = archive.manifest().entry.clone();
    let descriptor = archive.read(&entry)?;
    let decoded: CharacterTemplate = serde_json::from_slice(&descriptor)?;
    assert_eq!(decoded, template);

    let request = ContentHandlerRequest::new(CHARACTER_TEMPLATE_CONTENT_V1, descriptor);
    let response = validate_character_template_descriptor(&serde_json::to_vec(&request)?)?;
    let response: ContentHandlerResponse = serde_json::from_slice(&response)?;
    assert_eq!(response, ContentHandlerResponse::Accepted);
    Ok(())
}

#[test]
fn test_should_validate_character_template_through_generic_content_handler_contract()
-> anyhow::Result<()> {
    let template = import_tavern_v2(&serde_json::to_vec(&card_json())?)?;
    let descriptor = serde_json::to_vec(&template)?;
    let request = ContentHandlerRequest::new(CHARACTER_TEMPLATE_CONTENT_V1, descriptor);
    let response = validate_character_template_descriptor(&serde_json::to_vec(&request)?)?;
    let response: ContentHandlerResponse = serde_json::from_slice(&response)?;
    assert_eq!(response, ContentHandlerResponse::Accepted);

    let mut invalid = template;
    invalid.name.clear();
    let request =
        ContentHandlerRequest::new(CHARACTER_TEMPLATE_CONTENT_V1, serde_json::to_vec(&invalid)?);
    let response = validate_character_template_descriptor(&serde_json::to_vec(&request)?)?;
    let response: ContentHandlerResponse = serde_json::from_slice(&response)?;
    assert!(matches!(response, ContentHandlerResponse::Rejected { .. }));
    Ok(())
}

#[test]
fn test_should_build_runtime_neutral_identity_plan_without_session_or_narration_hints()
-> anyhow::Result<()> {
    let template = import_tavern_v2(&serde_json::to_vec(&card_json())?)?;
    let revision = ArtifactDigest::sha256(b"template revision");
    let instantiation =
        instantiate_character(&template, "018f0000-0000-7000-8000-000000000001", &revision)?;

    let schemas = character_world_schemas()?;
    assert_eq!(schemas.len(), 4);
    assert_eq!(schemas[0].kind(), SchemaKind::Entity);
    assert_eq!(schemas[1].kind(), SchemaKind::Facet);
    assert_eq!(schemas[2].kind(), SchemaKind::Command);
    assert_eq!(schemas[3].kind(), SchemaKind::Event);
    for schema in &schemas {
        let definition: Value = serde_json::from_str(schema.definition_json())?;
        assert_eq!(definition["type"], "object");
    }

    assert_eq!(instantiation.entity_schema, schemas[0].key().clone());
    assert_eq!(
        instantiation.identity_facet_schema,
        schemas[1].key().clone()
    );
    let identity = serde_json::to_value(&instantiation.identity)?;
    assert_eq!(identity["name"], "Alice");
    assert_eq!(identity["template-revision"], revision.to_string());
    assert!(identity.get("greeting").is_none());
    assert!(identity.get("system-prompt").is_none());
    assert!(identity.get("scenario").is_none());
    assert!(identity.get("personality").is_none());
    Ok(())
}

#[test]
fn test_should_allocate_independent_live_entity_ids_from_same_template() -> anyhow::Result<()> {
    let template: CharacterTemplate = import_tavern_v2(&serde_json::to_vec(&card_json())?)?;
    let revision = ArtifactDigest::sha256(b"same revision");
    let first = instantiate_character(&template, "library-id", &revision)?;
    let second = instantiate_character(&template, "library-id", &revision)?;
    assert_ne!(first.entity_id, second.entity_id);
    assert_eq!(first.identity, second.identity);
    assert_eq!(first.entity_schema, second.entity_schema);
    assert_eq!(first.identity_facet_schema, second.identity_facet_schema);
    Ok(())
}

fn png_header() -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let ihdr = [
        0, 0, 0, 1, // width
        0, 0, 0, 1, // height
        8, // bit depth
        6, // RGBA color type
        0, // compression
        0, // filter
        0, // interlace
    ];
    push_png_chunk(&mut png, b"IHDR", &ihdr);
    png
}

fn push_png_chunk(png: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(chunk_type);
    png.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(chunk_type);
    hasher.update(data);
    png.extend_from_slice(&hasher.finalize().to_be_bytes());
}

#[test]
fn test_should_build_deterministic_authoritative_character_transaction() -> anyhow::Result<()> {
    use rintawa_character_library::{
        CHARACTER_INSTANTIATE_COMMAND_SCHEMA, CharacterInstantiateCommand,
        build_character_instantiation_transaction, character_entity_schema_key,
        character_identity_facet_schema_key, character_instantiate_command_schema_key,
        character_instantiated_event_schema_key,
    };
    use rintawa_sdk::{
        world::CommandId,
        world_system::{WorldSystemFacetTarget, WorldSystemMutation},
    };

    let template = import_tavern_v2(&serde_json::to_vec(&card_json())?)?;
    let command_id = CommandId::new();
    let command = CharacterInstantiateCommand {
        template_id: String::from("character-template-id"),
        template_revision: ArtifactDigest::sha256(b"template").to_string(),
    };
    let transaction = build_character_instantiation_transaction(&template, &command, command_id)?;
    assert_eq!(transaction.mutations.len(), 2);
    let expected_entity = rintawa_sdk::world::EntityId::from_bytes(command_id.into_bytes());
    assert!(matches!(
        &transaction.mutations[0],
        WorldSystemMutation::CreateEntity { entity_id, schema }
            if *entity_id == expected_entity && schema == &character_entity_schema_key()?
    ));
    assert!(matches!(
        &transaction.mutations[1],
        WorldSystemMutation::SetFacet {
            target: WorldSystemFacetTarget::Entity(entity_id),
            schema,
            ..
        } if *entity_id == expected_entity && schema == &character_identity_facet_schema_key()?
    ));
    assert_eq!(transaction.events.len(), 1);
    assert_eq!(
        transaction.events[0].schema,
        character_instantiated_event_schema_key()?
    );
    assert_eq!(
        character_instantiate_command_schema_key()?.to_string(),
        CHARACTER_INSTANTIATE_COMMAND_SCHEMA
    );
    Ok(())
}
