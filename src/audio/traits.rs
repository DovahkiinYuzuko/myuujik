use std::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub trait AudioOutputBackend: Send {
    fn play(&mut self) -> Result<(), Box<dyn Error + Send + Sync>>;
    fn pause(&mut self) -> Result<(), Box<dyn Error + Send + Sync>>;
    fn is_active(&self) -> bool;
    fn mode_name(&self) -> &'static str;
}
