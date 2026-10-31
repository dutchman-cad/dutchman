//! Where a file that did not convert can be sent privately: the Discord invite, Dutchy's
//! account, Discord's upload limit, the private link's limits and the one-line message to
//! paste. One versioned policy holds them, bundled here; the desktop app may refresh it.
//!
//! The policy is about the service, never about a file: the file's details fill the message
//! in [`ReportPolicy::message`], on the reporter's machine, and only when they ask.

use std::sync::OnceLock;

use serde_json::Value;

/// The policy schema this build reads. A policy of any other version is not used.
pub const SCHEMA_VERSION: u64 = 1;

/// Where a newer copy of the policy is published. The request carries no query and nothing
/// about any file.
pub const REFRESH_URL: &str = "https://dutchman-cad.github.io/report-policy.json";

const BUNDLED: &str = include_str!("../assets/report-policy.json");

/// Discord counts its upload limit in binary megabytes, and so does the card.
const MEGABYTE: u64 = 1 << 20;

/// The longest error code a message carries.
const MAX_CODE: usize = 80;

/// The longest message template accepted.
const MAX_TEMPLATE: usize = 300;

/// The message template's fields.
const FIELDS: [&str; 5] = ["<version>", "<code>", "<id>", "<n>", "<yes|no|unknown>"];

/// What the private upload link accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateLink {
    pub design_mb: u64,
    pub step_mb: u64,
    pub zip_mb: u64,
    pub hours: u64,
}

/// One copy of the policy, checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportPolicy {
    pub version: u64,
    pub invite_url: String,
    pub dutchy_user_id: String,
    pub discord_upload_limit_mb: u64,
    pub private_link: PrivateLink,
    pub message_template: String,
}

/// What the message says about one failed file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageFacts<'a> {
    /// The error's code, if it has one.
    pub code: Option<&'a str>,
    /// The anonymous report's id for the file.
    pub report_id: &'a str,
    pub size_bytes: Option<u64>,
    /// Whether a STEP of the design is at hand, when that is known.
    pub step: Option<bool>,
}

/// The version the message names.
pub fn report_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

impl ReportPolicy {
    /// The copy built into this program.
    #[allow(clippy::expect_used)] // A test parses this same text.
    pub fn bundled() -> &'static ReportPolicy {
        static POLICY: OnceLock<ReportPolicy> = OnceLock::new();
        POLICY.get_or_init(|| ReportPolicy::parse(BUNDLED).expect("the bundled policy is valid"))
    }

    /// Reads a copy of the policy, refusing one of another schema version or with a value the
    /// app should not act on: it is only ever opened in a browser as a Discord link.
    pub fn parse(text: &str) -> Result<ReportPolicy, String> {
        let value: Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
        let number = |value: &Value, key: &str| {
            value
                .get(key)
                .and_then(Value::as_u64)
                .filter(|n| (1..=1_000_000).contains(n))
                .ok_or_else(|| format!("the policy has no usable {key}"))
        };
        let string = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("the policy has no {key}"))
        };
        let version = number(&value, "version")?;
        if version != SCHEMA_VERSION {
            return Err(format!("the policy's schema version is {version}"));
        }
        let link = value
            .get("privateLink")
            .ok_or("the policy has no privateLink")?;
        let policy = ReportPolicy {
            version,
            invite_url: string("inviteUrl")?,
            dutchy_user_id: string("dutchyUserId")?,
            discord_upload_limit_mb: number(&value, "discordUploadLimitMb")?,
            private_link: PrivateLink {
                design_mb: number(link, "designMb")?,
                step_mb: number(link, "stepMb")?,
                zip_mb: number(link, "zipMb")?,
                hours: number(link, "hours")?,
            },
            message_template: string("messageTemplate")?,
        };
        policy.check()?;
        Ok(policy)
    }

    fn check(&self) -> Result<(), String> {
        let invite = self
            .invite_url
            .strip_prefix("https://discord.gg/")
            .or_else(|| self.invite_url.strip_prefix("https://discord.com/invite/"))
            .ok_or("the policy's invite is not a Discord invite")?;
        if invite.is_empty() || !invite.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("the policy's invite is not a Discord invite".to_owned());
        }
        let id = &self.dutchy_user_id;
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("the policy's Dutchy id is not a Discord id".to_owned());
        }
        let template = &self.message_template;
        if template.len() > MAX_TEMPLATE || template.chars().any(char::is_control) {
            return Err("the policy's message is not one short line".to_owned());
        }
        if !FIELDS.iter().all(|field| template.contains(field)) {
            return Err("the policy's message lacks a field".to_owned());
        }
        let rest = FIELDS
            .iter()
            .fold(template.clone(), |text, field| text.replace(field, ""));
        if rest.contains(['<', '>']) {
            return Err("the policy's message has an unknown field".to_owned());
        }
        Ok(())
    }

    /// The link that opens Dutchy's profile, where a DM starts.
    pub fn dm_url(&self) -> String {
        format!("https://discord.com/users/{}", self.dutchy_user_id)
    }

    /// Whether a file of `size_bytes` can be dragged into a DM. A size not known takes the
    /// private link, which works for any size.
    pub fn fits_discord(&self, size_bytes: Option<u64>) -> bool {
        size_bytes.is_some_and(|size| size <= self.discord_upload_limit_mb.saturating_mul(MEGABYTE))
    }

    /// What the private link accepts, in words.
    pub fn link_limits(&self) -> String {
        let link = &self.private_link;
        format!(
            "{} for the design, {} for its STEP, or a {} zip, for {} hours",
            megabytes(link.design_mb),
            megabytes(link.step_mb),
            megabytes(link.zip_mb),
            link.hours
        )
    }

    /// The one-line message to paste into the DM.
    pub fn message(&self, facts: &MessageFacts<'_>) -> String {
        let size = facts
            .size_bytes
            .map_or_else(|| "unknown".to_owned(), size_text);
        let step = match facts.step {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        };
        self.message_template
            .replace("<version>", report_version())
            .replace("<code>", &code_token(facts.code))
            .replace("<id>", facts.report_id)
            .replace("<n>", &size)
            .replace("<yes|no|unknown>", step)
    }
}

/// `mb` megabytes as the card writes them: 300 MB, 1.5 GB.
fn megabytes(mb: u64) -> String {
    if mb < 1000 {
        return format!("{mb} MB");
    }
    match mb % 1000 / 100 {
        0 => format!("{} GB", mb / 1000),
        tenths => format!("{}.{tenths} GB", mb / 1000),
    }
}

/// `bytes` in megabytes: a whole number or one decimal, and never 0 for a file with data.
fn size_text(bytes: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + u128::from(MEGABYTE) / 2) / u128::from(MEGABYTE);
    let tenths = if bytes > 0 { tenths.max(1) } else { 0 };
    match tenths % 10 {
        0 => format!("{}", tenths / 10),
        rest => format!("{}.{rest}", tenths / 10),
    }
}

/// `code` as one token, or `unknown`. Anything but plain code characters is refused, not
/// cleaned up, so a message can never carry a sentence or a path.
fn code_token(code: Option<&str>) -> String {
    code.filter(|code| {
        !code.is_empty()
            && code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
    })
    .map_or_else(
        || "unknown".to_owned(),
        |code| code.chars().take(MAX_CODE).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    fn policy() -> ReportPolicy {
        ReportPolicy::bundled().clone()
    }

    fn valid() -> Value {
        serde_json::from_str(BUNDLED).unwrap()
    }

    fn keys(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn the_bundled_policy_has_exactly_the_schemas_fields_and_values() {
        let value = valid();
        assert_eq!(
            keys(&value),
            BTreeSet::from([
                "version",
                "inviteUrl",
                "dutchyUserId",
                "discordUploadLimitMb",
                "privateLink",
                "messageTemplate"
            ])
        );
        assert_eq!(
            keys(&value["privateLink"]),
            BTreeSet::from(["designMb", "stepMb", "zipMb", "hours"])
        );
        let policy = policy();
        assert_eq!(policy.version, SCHEMA_VERSION);
        assert_eq!(policy.invite_url, crate::json::DISCORD_URL);
        assert_eq!(policy.dutchy_user_id, "1563591265810129088");
        assert_eq!(
            policy.dm_url(),
            "https://discord.com/users/1563591265810129088"
        );
        assert_eq!(policy.discord_upload_limit_mb, 10);
        assert_eq!(
            policy.private_link,
            PrivateLink {
                design_mb: 300,
                step_mb: 300,
                zip_mb: 1536,
                hours: 72
            }
        );
    }

    #[test]
    fn the_message_is_exactly_one_line_in_the_agreed_words() {
        let message = policy().message(&MessageFacts {
            code: Some("no_geometry"),
            report_id: "f0011223344556677",
            size_bytes: Some(12_400_000),
            step: None,
        });
        assert_eq!(
            message,
            format!(
                "Dutchman {} couldn't convert my file. Error: no_geometry. Report id: \
                 f0011223344556677. Size: 11.8 MB. STEP: unknown.",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert!(!message.contains('\n'));
    }

    #[test]
    fn the_step_field_says_only_what_is_known() {
        let policy = policy();
        let said = |step| {
            policy.message(&MessageFacts {
                code: None,
                report_id: "f1",
                size_bytes: None,
                step,
            })
        };
        assert!(said(Some(true)).ends_with("Size: unknown MB. STEP: yes."));
        assert!(said(Some(false)).ends_with("STEP: no."));
        assert!(said(None).contains("Error: unknown."));
    }

    #[test]
    fn codes_are_single_tokens_of_at_most_80_characters() {
        assert_eq!(code_token(None), "unknown");
        assert_eq!(code_token(Some("")), "unknown");
        assert_eq!(code_token(Some("two words")), "unknown");
        assert_eq!(code_token(Some("/home/me/part.f3d")), "unknown");
        assert_eq!(
            code_token(Some("cadmpeg:bad-header_2.1")),
            "cadmpeg:bad-header_2.1"
        );
        assert_eq!(code_token(Some(&"x".repeat(200))).len(), 80);
    }

    #[test]
    fn sizes_are_whole_or_one_decimal() {
        assert_eq!(size_text(0), "0");
        assert_eq!(size_text(1), "0.1");
        assert_eq!(size_text(MEGABYTE), "1");
        assert_eq!(size_text(MEGABYTE * 3 / 2), "1.5");
        assert_eq!(size_text(MEGABYTE * 10), "10");
    }

    #[test]
    fn the_limit_is_inclusive_and_an_unknown_size_takes_the_link() {
        let policy = policy();
        assert!(policy.fits_discord(Some(10 * MEGABYTE)));
        assert!(!policy.fits_discord(Some(10 * MEGABYTE + 1)));
        assert!(!policy.fits_discord(None));
    }

    #[test]
    fn the_links_limits_read_in_words() {
        assert_eq!(
            policy().link_limits(),
            "300 MB for the design, 300 MB for its STEP, or a 1.5 GB zip, for 72 hours"
        );
        assert_eq!(megabytes(2000), "2 GB");
    }

    #[test]
    fn a_policy_of_another_schema_version_is_refused() {
        for version in [0, SCHEMA_VERSION + 1] {
            let mut value = valid();
            value["version"] = json!(version);
            assert!(ReportPolicy::parse(&value.to_string()).is_err());
        }
    }

    #[test]
    fn the_bundled_template_is_the_one_the_bot_parses() {
        assert_eq!(
            ReportPolicy::bundled().message_template,
            "Dutchman <version> couldn't convert my file. Error: <code>. Report id: <id>. \
             Size: <n> MB. STEP: <yes|no|unknown>."
        );
    }

    #[test]
    fn a_policy_the_app_should_not_act_on_is_refused() {
        for (key, bad) in [
            ("inviteUrl", json!("https://example.com/yTcseBEdeu")),
            ("inviteUrl", json!("https://discord.gg/a?b=c")),
            ("dutchyUserId", json!("../../evil")),
            ("discordUploadLimitMb", json!(0)),
            ("messageTemplate", json!("no fields at all")),
            (
                "messageTemplate",
                json!("<version><code><id><n><yes|no|unknown><path>"),
            ),
            (
                "messageTemplate",
                json!("<version><code><id><n>\n<yes|no|unknown>"),
            ),
        ] {
            let mut value = valid();
            value[key] = bad;
            assert!(ReportPolicy::parse(&value.to_string()).is_err(), "{key}");
        }
        assert!(ReportPolicy::parse("not json").is_err());
    }
}
