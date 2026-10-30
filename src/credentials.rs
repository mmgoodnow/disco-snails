use anyhow::{bail, Context, Result};

pub fn resolve_token(
    environment: Option<String>,
    keychain: impl FnOnce() -> Result<Option<String>>,
) -> Result<String> {
    if let Some(token) = environment.filter(|token| !token.trim().is_empty()) {
        return Ok(token);
    }
    keychain()?.context(
        "No Discord bot token configured; run `disco auth login` on macOS or set DISCORD_BOT_TOKEN in the environment or .env",
    )
}

fn clean_token(token: &str) -> Result<&str> {
    let token = token.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        bail!("Bot token must be nonempty and contain no whitespace");
    }
    Ok(token)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, generic_password, set_generic_password, PasswordOptions,
    };

    const SERVICE: &str = "disco";
    const ACCOUNT: &str = "discord-bot-token";
    const ITEM_NOT_FOUND: i32 = -25300;

    pub fn read_token() -> Result<Option<String>> {
        match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT)) {
            Ok(bytes) => {
                let token =
                    String::from_utf8(bytes).context("Keychain token is not valid UTF-8")?;
                Ok(Some(clean_token(&token)?.to_owned()))
            }
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(error).context("Could not read bot token from macOS Keychain"),
        }
    }

    pub fn save_token(token: &str) -> Result<()> {
        set_generic_password(SERVICE, ACCOUNT, clean_token(token)?.as_bytes())
            .context("Could not save bot token in macOS Keychain")
    }

    pub fn delete_token() -> Result<()> {
        match delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(error).context("Could not delete bot token from macOS Keychain"),
        }
    }

    pub fn ensure_supported() -> Result<()> {
        Ok(())
    }

    pub fn prompt_token() -> Result<String> {
        rpassword::prompt_password("Discord bot token: ").context("Could not read bot token")
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn read_token() -> Result<Option<String>> {
        Ok(None)
    }

    pub fn ensure_supported() -> Result<()> {
        bail!("Keychain token storage requires macOS; set DISCORD_BOT_TOKEN in the environment or .env")
    }

    pub fn save_token(token: &str) -> Result<()> {
        clean_token(token)?;
        ensure_supported()
    }

    pub fn delete_token() -> Result<()> {
        ensure_supported()
    }

    pub fn prompt_token() -> Result<String> {
        ensure_supported()?;
        unreachable!()
    }
}

pub use platform::{delete_token, ensure_supported, prompt_token, read_token, save_token};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_overrides_keychain_without_reading_it() {
        let token = resolve_token(Some("env-token".into()), || {
            panic!("must not read Keychain")
        })
        .unwrap();
        assert_eq!(token, "env-token");
    }

    #[test]
    fn empty_environment_falls_back_to_keychain() {
        for environment in [None, Some(" \n".into())] {
            assert_eq!(
                resolve_token(environment, || Ok(Some("saved-token".into()))).unwrap(),
                "saved-token"
            );
        }
    }

    #[test]
    fn missing_token_explains_setup() {
        let error = resolve_token(None, || Ok(None)).unwrap_err().to_string();
        assert!(error.contains("disco auth login"));
        assert!(error.contains("DISCORD_BOT_TOKEN"));
    }

    #[test]
    fn keychain_errors_are_preserved() {
        let error = resolve_token(None, || bail!("keychain locked")).unwrap_err();
        assert_eq!(error.to_string(), "keychain locked");
    }

    #[test]
    fn trims_pasted_token_and_rejects_invalid_input_without_echoing_it() {
        assert_eq!(clean_token(" token\r\n").unwrap(), "token");
        for token in ["", " \n", "secret\nother", "secret value"] {
            let error = clean_token(token).unwrap_err().to_string();
            assert!(!error.contains("secret"));
        }
    }
}
