use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::kernel::deepseek::{DeepSeekChatTelemetry, DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL};

pub const DEEPSEEK_PRICING_SETTINGS_FILE: &str = "deepseek-pricing.json";
const MICRO_USD_PER_USD: u64 = 1_000_000;
const TOKENS_PER_MILLION: u128 = 1_000_000;

#[derive(Debug, thiserror::Error)]
pub enum DeepSeekPricingError {
    #[error("deepseek pricing settings could not be read: {0}")]
    Read(std::io::Error),

    #[error("deepseek pricing settings could not be written: {0}")]
    Write(std::io::Error),

    #[error("deepseek pricing settings are invalid json: {0}")]
    Json(serde_json::Error),

    #[error("deepseek pricing value is invalid: {0}")]
    InvalidPrice(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekPricingSettings {
    pub enabled: bool,
    pub flash_prompt_usd_per_million_tokens: String,
    pub flash_completion_usd_per_million_tokens: String,
    pub pro_prompt_usd_per_million_tokens: String,
    pub pro_completion_usd_per_million_tokens: String,
}

impl Default for DeepSeekPricingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            flash_prompt_usd_per_million_tokens: String::new(),
            flash_completion_usd_per_million_tokens: String::new(),
            pro_prompt_usd_per_million_tokens: String::new(),
            pro_completion_usd_per_million_tokens: String::new(),
        }
    }
}

impl DeepSeekPricingSettings {
    pub fn normalized(mut self) -> Result<Self, DeepSeekPricingError> {
        self.flash_prompt_usd_per_million_tokens =
            self.flash_prompt_usd_per_million_tokens.trim().to_string();
        self.flash_completion_usd_per_million_tokens = self
            .flash_completion_usd_per_million_tokens
            .trim()
            .to_string();
        self.pro_prompt_usd_per_million_tokens =
            self.pro_prompt_usd_per_million_tokens.trim().to_string();
        self.pro_completion_usd_per_million_tokens = self
            .pro_completion_usd_per_million_tokens
            .trim()
            .to_string();

        self.validate()?;
        Ok(self)
    }

    pub fn has_any_rate(&self) -> bool {
        [
            &self.flash_prompt_usd_per_million_tokens,
            &self.flash_completion_usd_per_million_tokens,
            &self.pro_prompt_usd_per_million_tokens,
            &self.pro_completion_usd_per_million_tokens,
        ]
        .iter()
        .any(|value| !value.trim().is_empty())
    }

    fn validate(&self) -> Result<(), DeepSeekPricingError> {
        parse_usd_per_million_to_micro_usd_per_million(&self.flash_prompt_usd_per_million_tokens)?;
        parse_usd_per_million_to_micro_usd_per_million(
            &self.flash_completion_usd_per_million_tokens,
        )?;
        parse_usd_per_million_to_micro_usd_per_million(&self.pro_prompt_usd_per_million_tokens)?;
        parse_usd_per_million_to_micro_usd_per_million(
            &self.pro_completion_usd_per_million_tokens,
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekPricingState {
    pub settings: DeepSeekPricingSettings,
    pub pricing_configured: bool,
    pub note: String,
}

pub fn load_deepseek_pricing_state(
    app_data_dir: impl AsRef<Path>,
) -> Result<DeepSeekPricingState, DeepSeekPricingError> {
    let app_data_dir = app_data_dir.as_ref();
    let settings_file = app_data_dir.join(DEEPSEEK_PRICING_SETTINGS_FILE);
    let settings = if settings_file.exists() {
        let settings_json =
            fs::read_to_string(&settings_file).map_err(DeepSeekPricingError::Read)?;
        serde_json::from_str::<DeepSeekPricingSettings>(&settings_json)
            .map_err(DeepSeekPricingError::Json)?
            .normalized()?
    } else {
        DeepSeekPricingSettings::default()
    };

    Ok(deepseek_pricing_state_from_settings(settings))
}

pub fn save_deepseek_pricing_settings(
    app_data_dir: impl AsRef<Path>,
    settings: DeepSeekPricingSettings,
) -> Result<DeepSeekPricingState, DeepSeekPricingError> {
    let app_data_dir = app_data_dir.as_ref();
    let settings = settings.normalized()?;
    fs::create_dir_all(app_data_dir).map_err(DeepSeekPricingError::Write)?;
    let settings_file = app_data_dir.join(DEEPSEEK_PRICING_SETTINGS_FILE);
    let settings_json =
        serde_json::to_string_pretty(&settings).map_err(DeepSeekPricingError::Json)?;
    fs::write(&settings_file, settings_json).map_err(DeepSeekPricingError::Write)?;

    Ok(deepseek_pricing_state_from_settings(settings))
}

pub fn estimate_deepseek_chat_cost_micro_usd(
    telemetry: &DeepSeekChatTelemetry,
    settings: &DeepSeekPricingSettings,
) -> Option<u64> {
    try_estimate_deepseek_chat_cost_micro_usd(telemetry, settings)
        .ok()
        .flatten()
}

pub fn try_estimate_deepseek_chat_cost_micro_usd(
    telemetry: &DeepSeekChatTelemetry,
    settings: &DeepSeekPricingSettings,
) -> Result<Option<u64>, DeepSeekPricingError> {
    if !settings.enabled {
        return Ok(None);
    }

    let Some(prompt_tokens) = telemetry.prompt_tokens else {
        return Ok(None);
    };
    let Some(completion_tokens) = telemetry.completion_tokens else {
        return Ok(None);
    };
    let Some((prompt_rate, completion_rate)) = rate_for_model(&telemetry.model, settings)? else {
        return Ok(None);
    };

    let prompt_cost = prorate_micro_usd(prompt_tokens, prompt_rate);
    let completion_cost = prorate_micro_usd(completion_tokens, completion_rate);
    let total = prompt_cost.saturating_add(completion_cost);
    Ok(Some(total.min(u64::MAX as u128) as u64))
}

fn deepseek_pricing_state_from_settings(settings: DeepSeekPricingSettings) -> DeepSeekPricingState {
    let pricing_configured = settings.enabled && settings.has_any_rate();
    DeepSeekPricingState {
        settings,
        pricing_configured,
        note: if pricing_configured {
            "manual DeepSeek pricing is configured for local cost estimates".to_string()
        } else {
            "DeepSeek cost estimates are disabled until a local pricing table is configured"
                .to_string()
        },
    }
}

fn rate_for_model(
    model: &str,
    settings: &DeepSeekPricingSettings,
) -> Result<Option<(u64, u64)>, DeepSeekPricingError> {
    let rates = match model {
        DEEPSEEK_FLASH_MODEL => (
            &settings.flash_prompt_usd_per_million_tokens,
            &settings.flash_completion_usd_per_million_tokens,
        ),
        DEEPSEEK_PRO_MODEL => (
            &settings.pro_prompt_usd_per_million_tokens,
            &settings.pro_completion_usd_per_million_tokens,
        ),
        _ => return Ok(None),
    };

    let Some(prompt_rate) = parse_usd_per_million_to_micro_usd_per_million(rates.0)? else {
        return Ok(None);
    };
    let Some(completion_rate) = parse_usd_per_million_to_micro_usd_per_million(rates.1)? else {
        return Ok(None);
    };
    Ok(Some((prompt_rate, completion_rate)))
}

fn parse_usd_per_million_to_micro_usd_per_million(
    value: &str,
) -> Result<Option<u64>, DeepSeekPricingError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.starts_with('-') {
        return Err(DeepSeekPricingError::InvalidPrice(
            "prices must be zero or positive".to_string(),
        ));
    }

    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() {
        return Err(DeepSeekPricingError::InvalidPrice(
            "prices must be decimal numbers".to_string(),
        ));
    }
    if whole.is_empty() && fraction.is_none_or(str::is_empty) {
        return Err(DeepSeekPricingError::InvalidPrice(
            "prices must include digits".to_string(),
        ));
    }
    if !whole.chars().all(|character| character.is_ascii_digit()) {
        return Err(DeepSeekPricingError::InvalidPrice(
            "prices must use digits and a decimal point".to_string(),
        ));
    }

    let whole_micro = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u64>()
            .map_err(|_| DeepSeekPricingError::InvalidPrice("price is too large".to_string()))?
            .checked_mul(MICRO_USD_PER_USD)
            .ok_or_else(|| DeepSeekPricingError::InvalidPrice("price is too large".to_string()))?
    };

    let fraction_micro = match fraction {
        Some(fraction) => {
            if fraction.len() > 6 {
                return Err(DeepSeekPricingError::InvalidPrice(
                    "prices support up to 6 decimal places".to_string(),
                ));
            }
            if !fraction.chars().all(|character| character.is_ascii_digit()) {
                return Err(DeepSeekPricingError::InvalidPrice(
                    "prices must use digits and a decimal point".to_string(),
                ));
            }
            let mut padded = fraction.to_string();
            while padded.len() < 6 {
                padded.push('0');
            }
            padded.parse::<u64>().map_err(|_| {
                DeepSeekPricingError::InvalidPrice("price fraction is invalid".to_string())
            })?
        }
        None => 0,
    };

    whole_micro
        .checked_add(fraction_micro)
        .ok_or_else(|| DeepSeekPricingError::InvalidPrice("price is too large".to_string()))
        .map(Some)
}

fn prorate_micro_usd(tokens: u32, micro_usd_per_million_tokens: u64) -> u128 {
    let numerator = tokens as u128 * micro_usd_per_million_tokens as u128;
    numerator.div_ceil(TOKENS_PER_MILLION)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::{
        estimate_deepseek_chat_cost_micro_usd, load_deepseek_pricing_state,
        save_deepseek_pricing_settings, try_estimate_deepseek_chat_cost_micro_usd,
        DeepSeekPricingSettings, DEEPSEEK_PRICING_SETTINGS_FILE,
    };
    use crate::kernel::deepseek::{
        DeepSeekChatCacheStatus, DeepSeekChatTelemetry, DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL,
    };

    #[test]
    fn missing_deepseek_pricing_settings_defaults_to_disabled() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = load_deepseek_pricing_state(temp_dir.path()).expect("state loads");

        assert!(!state.settings.enabled);
        assert!(!state.pricing_configured);
        let state_json = serde_json::to_string(&state).expect("state serializes");
        assert!(!state_json.contains(&temp_dir.path().to_string_lossy().to_string()));
        assert!(!state_json.contains("app_data_dir"));
        assert!(!state_json.contains("settings_file"));
    }

    #[test]
    fn save_then_load_deepseek_pricing_settings_trims_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let saved = save_deepseek_pricing_settings(
            temp_dir.path(),
            DeepSeekPricingSettings {
                enabled: true,
                flash_prompt_usd_per_million_tokens: " 0.14 ".to_string(),
                flash_completion_usd_per_million_tokens: " 0.28 ".to_string(),
                pro_prompt_usd_per_million_tokens: " 0.55 ".to_string(),
                pro_completion_usd_per_million_tokens: " 2.19 ".to_string(),
            },
        )
        .expect("settings save");

        assert!(saved.pricing_configured);
        assert_eq!(saved.settings.flash_prompt_usd_per_million_tokens, "0.14");
        assert!(temp_dir
            .path()
            .join(DEEPSEEK_PRICING_SETTINGS_FILE)
            .is_file());

        let loaded = load_deepseek_pricing_state(temp_dir.path()).expect("state reloads");
        assert_eq!(loaded, saved);
    }

    #[test]
    fn deepseek_pricing_rejects_invalid_decimal_values() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let error = save_deepseek_pricing_settings(
            temp_dir.path(),
            DeepSeekPricingSettings {
                enabled: true,
                flash_prompt_usd_per_million_tokens: "0.1234567".to_string(),
                ..DeepSeekPricingSettings::default()
            },
        )
        .expect_err("too many decimals should fail");

        assert!(error.to_string().contains("up to 6 decimal places"));
    }

    #[test]
    fn estimates_deepseek_chat_cost_from_matching_model_rates() {
        let settings = DeepSeekPricingSettings {
            enabled: true,
            flash_prompt_usd_per_million_tokens: "0.14".to_string(),
            flash_completion_usd_per_million_tokens: "0.28".to_string(),
            ..DeepSeekPricingSettings::default()
        };
        let telemetry = telemetry_for_model(DEEPSEEK_FLASH_MODEL, Some(1_000_000), Some(500_000));

        let cost = try_estimate_deepseek_chat_cost_micro_usd(&telemetry, &settings)
            .expect("estimate works");

        assert_eq!(cost, Some(280_000));
    }

    #[test]
    fn skips_deepseek_chat_cost_when_disabled_or_rate_missing() {
        let disabled = DeepSeekPricingSettings {
            enabled: false,
            flash_prompt_usd_per_million_tokens: "0.14".to_string(),
            flash_completion_usd_per_million_tokens: "0.28".to_string(),
            ..DeepSeekPricingSettings::default()
        };
        let missing_pro_rate = DeepSeekPricingSettings {
            enabled: true,
            flash_prompt_usd_per_million_tokens: "0.14".to_string(),
            flash_completion_usd_per_million_tokens: "0.28".to_string(),
            ..DeepSeekPricingSettings::default()
        };
        let telemetry = telemetry_for_model(DEEPSEEK_PRO_MODEL, Some(100), Some(100));

        assert_eq!(
            estimate_deepseek_chat_cost_micro_usd(&telemetry, &disabled),
            None
        );
        assert_eq!(
            estimate_deepseek_chat_cost_micro_usd(&telemetry, &missing_pro_rate),
            None
        );
    }

    fn telemetry_for_model(
        model: &str,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    ) -> DeepSeekChatTelemetry {
        DeepSeekChatTelemetry {
            id: Uuid::new_v4(),
            request_hash: "abc123".to_string(),
            model: model.to_string(),
            cache_status: DeepSeekChatCacheStatus::Miss,
            elapsed_ms: 42,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens
                .zip(completion_tokens)
                .map(|(prompt, completion)| prompt + completion),
            estimated_cost_micro_usd: None,
            created_at: Utc::now(),
        }
    }
}
