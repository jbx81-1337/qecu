use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowInput {
    pub format: String,
    pub path: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowMemMap {
    pub from: u64,
    pub size: usize,
    pub flags: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowRegister {
    pub name: String,
    pub value: u64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowBoot {
    pub begin: u64,
    pub until: u64,
    pub timeout: u64,
    pub count: u64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workflow {
    pub project: String,
    pub input: WorkflowInput,
    pub mem_map: Vec<WorkflowMemMap>,
    pub registers: Vec<WorkflowRegister>,
    // pub boot: WorkflowBoot,
    pub init_script: String,
    pub sleigh_path: String
}

impl Workflow {
    pub fn new(content: String) -> Workflow {
        let content_str = content.as_str();
        let schema = serde_yaml::from_str::<Workflow>(content_str).unwrap();
        return schema;
    }
}