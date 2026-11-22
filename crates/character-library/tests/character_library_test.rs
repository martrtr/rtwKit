//! Integration tests for Character Library import, export, portrait binding and packaging.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::ZlibEncoder};
use rintawa_artifacts::{AssetStore, RtwArchive, RtwLimits};
use rintawa_character_library::{
    CHARACTER_TEMPLATE_CONTENT_V1, CHARACTER_TEMPLATE_ENTRY_PATH, CharacterTemplate,
    MAX_CHARACTER_TEMPLATE_BYTES, TavernV2Error, decode_tavern_v2_source,
    encode_character_template_rtw, export_tavern_v2_json, import_tavern_v2,
    import_tavern_v2_with_artwork, normalize_tavern_v2_source, pack_character_template_rtw,
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
fn test_should_pack_large_bounded_tavern_compatibility_payload() -> anyhow::Result<()> {
    let mut card = card_json();
    card["data"]["extensions"]["vendor/large"] = Value::String("x".repeat(700 * 1024));
    let template = import_tavern_v2(&serde_json::to_vec(&card)?)?;
    let descriptor = serde_json::to_vec_pretty(&template)?;

    assert!(descriptor.len() > 128 * 1024);
    assert!(descriptor.len() <= MAX_CHARACTER_TEMPLATE_BYTES);
    let rtw = encode_character_template_rtw(&template)?;
    assert!(!rtw.is_empty());
    Ok(())
}

#[test]
fn test_should_import_character_card_v3_json_and_png_metadata() -> anyhow::Result<()> {
    let mut v3 = card_json();
    v3["spec"] = Value::String(String::from("chara_card_v3"));
    v3["spec_version"] = Value::String(String::from("3.0"));
    let source = serde_json::to_vec(&v3)?;
    let template = import_tavern_v2(&source)?;
    assert_eq!(template.name, "Alice");
    assert_eq!(
        template
            .compatibility
            .tavern_v2
            .as_ref()
            .map(|compatibility| compatibility.spec_version.as_str()),
        Some("3.0")
    );

    let encoded = BASE64.encode(&source);
    let mut text = b"ccv3\0".to_vec();
    text.extend_from_slice(encoded.as_bytes());
    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);
    let template = import_tavern_v2(&png)?;
    assert_eq!(template.name, "Alice");
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
fn test_should_stage_transport_decode_before_character_normalization() -> anyhow::Result<()> {
    let mut card = card_json();
    card["data"]["extensions"]["vendor/large"] = Value::String("x".repeat(700 * 1024));
    let json = serde_json::to_vec(&card)?;
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(BASE64.encode(json).as_bytes());
    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);

    let staged = normalize_tavern_v2_source(decode_tavern_v2_source(&png)?)?;
    let direct = import_tavern_v2_with_artwork(&png)?;
    assert_eq!(staged, direct);
    assert_eq!(staged.template.name, "Alice");
    assert!(staged.portrait.is_some());
    Ok(())
}

#[test]
fn test_should_use_last_same_version_png_metadata_chunk() -> anyhow::Result<()> {
    let first = serde_json::to_vec(&card_json())?;
    let mut second_card = card_json();
    second_card["data"]["name"] = Value::String(String::from("Alice updated"));
    second_card["data"]["character_version"] = Value::String(String::from("8"));
    let second = serde_json::to_vec(&second_card)?;

    let mut first_text = b"chara\0".to_vec();
    first_text.extend_from_slice(BASE64.encode(first).as_bytes());
    let mut second_text = b"chara\0".to_vec();
    second_text.extend_from_slice(BASE64.encode(second).as_bytes());

    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", &first_text);
    push_png_chunk(&mut png, b"tEXt", &second_text);
    push_png_chunk(&mut png, b"IEND", &[]);

    let template = import_tavern_v2(&png)?;
    assert_eq!(template.name, "Alice updated");
    assert_eq!(
        template
            .compatibility
            .tavern_v2
            .as_ref()
            .map(|value| value.spec_version.as_str()),
        Some("2.0")
    );
    Ok(())
}

#[test]
fn test_should_extract_and_bind_png_portrait_through_asset_store() -> anyhow::Result<()> {
    let json = serde_json::to_vec(&card_json())?;
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(BASE64.encode(json).as_bytes());

    let mut png = png_header();
    push_png_chunk(&mut png, b"tEXt", b"note\0preserve-me");
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);

    let imported = import_tavern_v2_with_artwork(&png)?;
    assert!(imported.template.assets.portrait.is_none());
    let artwork = imported
        .portrait
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("PNG import must expose portrait artwork"))?;
    assert_eq!(artwork.media_type(), "image/png");
    assert!(
        artwork
            .bytes()
            .windows(b"preserve-me".len())
            .any(|part| part == b"preserve-me")
    );
    assert!(
        !artwork
            .bytes()
            .windows(b"chara\0".len())
            .any(|part| part == b"chara\0")
    );

    let root = tempfile::TempDir::new()?;
    let store = AssetStore::open(root.path().join("assets"), 16 * 1024 * 1024)?;
    let published = store.import_bytes(artwork.bytes(), artwork.media_type())?;
    let portrait = published.asset_ref().clone();
    store.verify(&portrait)?;

    let template = imported.clone().bind_portrait(portrait.clone())?;
    assert_eq!(template.assets.portrait.as_ref(), Some(&portrait));

    let wrong = store.import_bytes(b"different", "image/png")?;
    assert!(matches!(
        imported.bind_portrait(wrong.asset_ref().clone()),
        Err(TavernV2Error::PortraitAssetMismatch)
    ));

    Ok(())
}

#[test]
fn test_should_reject_portrait_binding_for_json_only_import() -> anyhow::Result<()> {
    let imported = import_tavern_v2_with_artwork(&serde_json::to_vec(&card_json())?)?;
    let root = tempfile::TempDir::new()?;
    let store = AssetStore::open(root.path().join("assets"), 1024)?;
    let portrait = store.import_bytes(b"not-a-png", "image/png")?;
    assert!(matches!(
        imported.bind_portrait(portrait.asset_ref().clone()),
        Err(TavernV2Error::MissingPngArtwork)
    ));
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
    let memory = encode_character_template_rtw(&template)?;
    assert_eq!(memory, std::fs::read(&output)?);

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
    let mut image = ZlibEncoder::new(Vec::new(), Compression::default());
    image.write_all(&[0, 0x10, 0x20, 0x30, 0xff]).unwrap();
    push_png_chunk(&mut png, b"IDAT", &image.finish().unwrap());
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
fn test_v3_source_exported_as_v2_has_a_valid_matching_envelope() -> anyhow::Result<()> {
    for version in ["3.0", "3.0.0"] {
        let mut source = card_json();
        source["spec"] = json!("chara_card_v3");
        source["spec_version"] = json!(version);
        source["data"]["group_only_greetings"] = json!(["Group greeting"]);
        source["data"]["assets"] = json!([{
            "type": "icon", "uri": "embed://portrait.png", "name": "main", "ext": "png"
        }]);
        let template = import_tavern_v2(&serde_json::to_vec(&source)?)?;
        let exported = export_tavern_v2_json(&template)?;
        let document: Value = serde_json::from_slice(&exported)?;
        assert_eq!(document["spec"], "chara_card_v2");
        assert_eq!(document["spec_version"], "2.0");
        assert_eq!(document["data"]["assets"], source["data"]["assets"]);
        assert_eq!(
            document["data"]["group_only_greetings"],
            source["data"]["group_only_greetings"]
        );
        assert_eq!(document["data"]["extensions"], source["data"]["extensions"]);
        let reimported = import_tavern_v2(&exported)?;
        assert_eq!(reimported.name, template.name);
        assert_eq!(reimported.narration, template.narration);
        // Exporting V2 does not mutate the original card's recorded source version.
        assert_eq!(
            template
                .compatibility
                .tavern_v2
                .as_ref()
                .unwrap()
                .spec_version,
            version
        );
    }
    Ok(())
}

#[test]
fn test_v3_version_checks_every_numeric_component() -> anyhow::Result<()> {
    for version in ["3.bad", "3.", "3..0", "3.-1", "3.bad.0", "3.0.bad"] {
        let mut source = card_json();
        source["spec"] = json!("chara_card_v3");
        source["spec_version"] = json!(version);
        assert!(matches!(
            import_tavern_v2(&serde_json::to_vec(&source)?),
            Err(TavernV2Error::UnsupportedVersion(_))
        ));
    }
    Ok(())
}

#[test]
fn test_png_prefers_v3_card_and_removes_both_card_chunks_from_portrait() -> anyhow::Result<()> {
    let v2 = serde_json::to_vec(&card_json())?;
    let mut v3 = card_json();
    v3["spec"] = json!("chara_card_v3");
    v3["spec_version"] = json!("3.0");
    v3["data"]["name"] = json!("V3 character");
    let v3 = serde_json::to_vec(&v3)?;
    let mut v2_text = b"chara\0".to_vec();
    v2_text.extend_from_slice(BASE64.encode(v2).as_bytes());
    let mut v3_text = b"ccv3\0".to_vec();
    v3_text.extend_from_slice(BASE64.encode(v3).as_bytes());
    for chunks in [[&v2_text, &v3_text], [&v3_text, &v2_text]] {
        let mut png = png_header();
        for chunk in chunks {
            push_png_chunk(&mut png, b"tEXt", chunk);
        }
        push_png_chunk(&mut png, b"IEND", &[]);
        let imported = import_tavern_v2_with_artwork(&png)?;
        assert_eq!(imported.template.name, "V3 character");
        let portrait = imported.portrait.unwrap();
        assert!(matches!(
            import_tavern_v2(portrait.bytes()),
            Err(TavernV2Error::MissingPngCardPayload)
        ));
    }
    Ok(())
}

fn png_card_with_dimensions(width: u32, height: u32) -> Vec<u8> {
    let mut png = png_header();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32fast::hash(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    let mut text = b"chara\0".to_vec();
    text.extend_from_slice(
        BASE64
            .encode(serde_json::to_vec(&card_json()).unwrap())
            .as_bytes(),
    );
    push_png_chunk(&mut png, b"tEXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);
    png
}

#[test]
fn test_portrait_header_bounds_without_claiming_pixel_stream_validation() {
    for (width, height) in [(4096, 1), (1, 4096), (2000, 2000)] {
        assert!(import_tavern_v2(&png_card_with_dimensions(width, height)).is_ok());
    }
    for (width, height) in [(4097, 1), (1, 4097), (2001, 2000), (0, 1), (1, 0)] {
        assert!(matches!(
            import_tavern_v2(&png_card_with_dimensions(width, height)),
            Err(TavernV2Error::InvalidPng(
                "portrait dimensions exceed package bounds"
            ))
        ));
    }
}

#[test]
fn test_animated_png_is_rejected_instead_of_silently_stripping_animation() {
    for kind in [b"acTL", b"fcTL", b"fdAT"] {
        let mut png = png_header();
        push_png_chunk(&mut png, kind, &[0; 8]);
        push_png_chunk(&mut png, b"IEND", &[]);
        assert!(matches!(
            import_tavern_v2(&png),
            Err(TavernV2Error::InvalidPng("animated PNG is not supported"))
        ));
    }
}

#[test]
fn test_card_metadata_decompression_stops_at_the_output_bound() -> anyhow::Result<()> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&vec![
        b'a';
        rintawa_character_library::MAX_TAVERN_CARD_BYTES + 1
    ])?;
    let mut text = b"chara\0\0".to_vec();
    text.extend_from_slice(&encoder.finish()?);
    let mut png = png_header();
    push_png_chunk(&mut png, b"zTXt", &text);
    push_png_chunk(&mut png, b"IEND", &[]);
    assert!(matches!(
        import_tavern_v2(&png),
        Err(TavernV2Error::InputTooLarge)
    ));
    Ok(())
}
