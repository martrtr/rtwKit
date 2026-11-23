use serde::Deserialize;

const MAX_CONTRIBUTIONS: usize = 16;
const MAX_CONTRIBUTION_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProvidedContract {
    pub id: String,
    pub version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub point: String,
    pub payload: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Descriptor {
    pub entry: String,
    pub title: String,
    #[serde(default)]
    pub host_shell: bool,
    #[serde(default = "launchable_by_default")]
    pub launchable: bool,
    #[serde(default)]
    pub provides: Vec<ProvidedContract>,
    #[serde(default)]
    pub contributions: Vec<Contribution>,
}

fn launchable_by_default() -> bool {
    true
}

// Keep the declaration grammar in sync with Core's validate_transport_point.
fn valid_point(point: &str) -> bool {
    point.len() <= 256
        && point.split('/').count() <= 5
        && point.split('/').all(|segment| {
            segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                && segment.len() <= 64
                && !segment.contains("..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

impl Descriptor {
    pub fn parse(bytes: &[u8]) -> Result<Self, ()> {
        let descriptor: Self =
            toml::from_str(std::str::from_utf8(bytes).map_err(|_| ())?).map_err(|_| ())?;
        if descriptor.title.trim().is_empty()
            || descriptor.title.chars().count() > 128
            || descriptor.provides.len() > 8
            || descriptor.contributions.len() > MAX_CONTRIBUTIONS
            || descriptor
                .contributions
                .iter()
                .map(|c| c.payload.len())
                .sum::<usize>()
                > MAX_CONTRIBUTION_BYTES
        {
            return Err(());
        }
        let mut points = std::collections::HashSet::new();
        if !descriptor
            .contributions
            .iter()
            .all(|c| valid_point(&c.point) && points.insert(&c.point))
        {
            return Err(());
        }
        Ok(descriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "entry = 'web/index.html'\ntitle = 'View'\n";

    #[test]
    fn descriptor_accepts_opaque_utf8_and_has_explicit_launchability() {
        let default = Descriptor::parse(BASE.as_bytes()).unwrap();
        assert!(default.launchable);
        assert_eq!(default.title, "View");
        assert_eq!(default.entry, "web/index.html");
        assert!(!default.host_shell);
        let text = format!(
            "{BASE}launchable = false\n[[provides]]\nid = 'test.renderer'\nversion = 1\n[[contributions]]\npoint = 'foo/bar-v1'\npayload = '☀ opaque bytes'"
        );
        let value = Descriptor::parse(text.as_bytes()).unwrap();
        assert!(!value.launchable);
        assert_eq!(value.provides[0].id, "test.renderer");
        assert_eq!(value.provides[0].version, 1);
        assert_eq!(value.contributions[0].payload, "☀ opaque bytes");
    }

    #[test]
    fn descriptor_rejects_unknown_fields_and_invalid_transport_points() {
        for fields in [
            "settings = true",
            "[[contributions]]\npoint = 'a'\npayload = 'b'\nowner = 'forged'",
            "[[contributions]]\npoint = 'a/*'\npayload = 'b'",
            "[[contributions]]\npoint = 'a/a..b'\npayload = 'b'",
            "[[contributions]]\npoint = 'a/b/c/d/e/f'\npayload = 'b'",
            "[[contributions]]\npoint = 'a//b'\npayload = 'b'",
            "[[contributions]]\npoint = '/a'\npayload = 'b'",
            "[[contributions]]\npoint = 'a'\npayload = 'b'\n[[contributions]]\npoint = 'a'\npayload = 'c'",
        ] {
            assert!(
                Descriptor::parse(format!("{BASE}{fields}").as_bytes()).is_err(),
                "{fields}"
            );
        }
    }

    #[test]
    fn descriptor_bounds_count_and_total_payload_bytes() {
        let mut descriptor = BASE.to_owned();
        let payload = "é".repeat(MAX_CONTRIBUTION_BYTES / 2 / MAX_CONTRIBUTIONS);
        for index in 0..MAX_CONTRIBUTIONS {
            descriptor.push_str(&format!(
                "[[contributions]]\npoint = 'point/{index}'\npayload = '{payload}'\n"
            ));
        }
        assert!(Descriptor::parse(descriptor.as_bytes()).is_ok());
        let oversized = descriptor.replacen(&payload, &format!("{payload}x"), 1);
        assert!(Descriptor::parse(oversized.as_bytes()).is_err());
        descriptor.push_str("[[contributions]]\npoint = 'point/extra'\npayload = ''");
        assert!(Descriptor::parse(descriptor.as_bytes()).is_err());
    }
}
