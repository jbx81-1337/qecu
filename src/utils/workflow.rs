use serde::{Serialize, Deserialize};
use crate::qecu::channel::ChannelConfig;

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
    pub until: Option<u64>,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>
}

impl Workflow {
    pub fn new(content: String) -> Workflow {
        let content_str = content.as_str();
        let schema = serde_yaml::from_str::<Workflow>(content_str).unwrap();
        return schema;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channels_with_defaults() {
        let yaml = r#"
project: test
input:
mem_map:
  - from: 0x0
    size: 0x1000
    flags: RW
registers:
  - name: PC
    value: 0x0
init_script: ./plugin.wasm
channels:
  - name: fastboot
    transport: zeromq
    endpoint: ipc:///tmp/qecu-pixel9-fastboot.sock
    max_frame_bytes: 1048576
    queue_depth: 32
"#;
        let wf = Workflow::new(yaml.to_string());
        assert_eq!(wf.channels.len(), 1);
        assert_eq!(wf.channels[0].name, "fastboot");
        assert_eq!(wf.channels[0].transport, "zeromq");
        assert_eq!(wf.channels[0].max_frame_bytes, 1048576);
        assert_eq!(wf.channels[0].queue_depth, 32);
    }

    #[test]
    fn no_channels_defaults_to_empty() {
        let yaml = r#"
project: test
input:
mem_map:
  - from: 0x0
    size: 0x1000
    flags: RW
registers:
  - name: PC
    value: 0x0
init_script: ./plugin.wasm
"#;
        let wf = Workflow::new(yaml.to_string());
        assert!(wf.channels.is_empty());
    }
}