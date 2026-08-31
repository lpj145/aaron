use crate::Uuid;

#[derive(Clone)]
pub enum NodeEvents {
    StartService {
        name: String
    },
    BindClusterId {
        cluster_id: Uuid
    }
}