use crate::{admin::Admin, config::Config, db::Db};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    pub admin: Arc<Admin>,
    pub ai_tx: Option<mpsc::Sender<crate::ai::AiJob>>,
    pub assistant: Option<crate::assistant::AssistantService>,
    pub geo_slots: Arc<Semaphore>,
    pub http: reqwest::Client,
    pub api_windows: Arc<Mutex<HashMap<IpAddr, (i64, u16)>>>,
    pub assistant_windows: Arc<Mutex<HashMap<IpAddr, (i64, u16)>>>,
}

impl AppState {
    pub fn new(
        config: Config,
        admin_config: Option<crate::config::AdminConfig>,
        ai_tx: Option<mpsc::Sender<crate::ai::AiJob>>,
    ) -> anyhow::Result<Self> {
        let db = Db::new(config.database.clone(), 4);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .user_agent("FroklaWorkshop/2.0")
            .build()?;
        Ok(Self {
            config: Arc::new(config),
            db,
            admin: Arc::new(Admin::new(admin_config)),
            ai_tx,
            assistant: None,
            geo_slots: Arc::new(Semaphore::new(2)),
            http: client,
            api_windows: Arc::new(Mutex::new(HashMap::new())),
            assistant_windows: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn with_assistant(mut self, assistant: crate::assistant::AssistantService) -> Self {
        self.assistant = Some(assistant);
        self
    }
}
