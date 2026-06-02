use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Healthy,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapability {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub state: RuntimeState,
    pub pack_version: Option<String>,
    pub repair_needed: bool,
    pub repair_available: bool,
    pub summary: String,
    pub blocked_capabilities: Vec<RuntimeCapability>,
    pub details: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<String>,
    #[serde(default)]
    pub bootstrap_eligible: bool,
    #[serde(default)]
    pub bootstrap_required: bool,
    #[serde(default)]
    pub active_operation: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPlanSource {
    ManagedReady,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPlan {
    pub eligible: bool,
    pub required: bool,
    pub source: Option<BootstrapPlanSource>,
    pub pack_version: Option<String>,
    pub summary: String,
    pub reason: Option<String>,
    pub remote_source: Option<serde_json::Value>,
    pub download: Option<serde_json::Value>,
}

fn lite_runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        state: RuntimeState::Healthy,
        pack_version: Some("lite-remote".to_string()),
        repair_needed: false,
        repair_available: false,
        summary: "EntropIA Lite usa proveedores remotos de IA".to_string(),
        blocked_capabilities: vec![],
        details: vec![
            "No se requiere instalación adicional de runtime de IA en este perfil.".to_string(),
        ],
        guidance: vec!["Configurá las claves remotas en Ajustes para usar IA.".to_string()],
        bootstrap_eligible: false,
        bootstrap_required: false,
        active_operation: None,
    }
}

fn lite_bootstrap_plan() -> BootstrapPlan {
    BootstrapPlan {
        eligible: false,
        required: false,
        source: Some(BootstrapPlanSource::ManagedReady),
        pack_version: Some("lite-remote".to_string()),
        summary: "EntropIA Lite no necesita bootstrap de runtime de IA".to_string(),
        reason: None,
        remote_source: None,
        download: None,
    }
}

#[tauri::command]
pub fn runtime_get_status(_app_handle: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    Ok(lite_runtime_status())
}

#[tauri::command]
pub fn runtime_get_bootstrap_plan(_app_handle: tauri::AppHandle) -> Result<BootstrapPlan, String> {
    Ok(lite_bootstrap_plan())
}

#[tauri::command]
pub fn runtime_repair(_app_handle: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    Ok(lite_runtime_status())
}
