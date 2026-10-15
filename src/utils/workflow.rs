use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowInput {
    pub format: String,
    #[serde(default)]
    pub path: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowMemMap {
    pub from: u64,
    pub size: u64,
    pub flags: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub content_address: Option<u64>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowRegister {
    pub name: String,
    pub value: u64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowBoot {
    #[serde(default)]
    pub begin: Option<u64>,
    #[serde(default)]
    pub until: Option<u64>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub count: Option<u64>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub project: String,
    #[serde(default)]
    pub input: Option<WorkflowInput>,
    pub mem_map: Vec<WorkflowMemMap>,
    pub registers: Vec<WorkflowRegister>,
    #[serde(default)]
    pub boot: Option<WorkflowBoot>,
    pub init_script: String,
    #[serde(default)]
    pub entry: Option<u64>,
    #[serde(default)]
    pub until: Option<u64>
}

impl Workflow {
    pub fn new(content: String) -> Workflow {
        let content_str = content.as_str();
        let schema = serde_yaml::from_str::<Workflow>(content_str).unwrap();
        return schema;
    }
}