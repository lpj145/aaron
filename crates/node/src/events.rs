use crate::Uuid;

#[derive(Clone, Debug)]
pub enum NodeEvents {
    StartService {
        name: String,
    },
    BindClusterId {
        cluster_id: Uuid,
    },
}

#[derive(Clone, Debug)]
pub struct SetEnvVar {
    pub key: String,
    pub value: String,
}