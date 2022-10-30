use serde::{Deserialize, Serialize};
use swc_core::ecma::atoms::JsWord;

/// Static plugin configuration.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub atom_names: Vec<JsWord>,
}

pub fn parse_plugin_config(plugin_str: &str) -> Config {
    serde_json::from_str::<Config>(plugin_str).expect("Invalid plugin config")
}
