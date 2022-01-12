use std::{borrow::Cow, collections::HashMap};

pub trait FieldMap {
    fn get_field(&self, key: &str) -> anyhow::Result<Cow<str>>;
}

impl FieldMap for HashMap<String, String> {
    fn get_field(&self, key: &str) -> anyhow::Result<Cow<str>> {
        self.get(key)
            .map(|s| Cow::Borrowed(s.as_str()))
            .ok_or_else(|| anyhow!("Missing field {}", key))
    }
}

impl FieldMap for mlua::Table<'_> {
    fn get_field(&self, key: &str) -> anyhow::Result<Cow<str>> {
        self.get::<&str, String>(key)
            .map(|s| Cow::Owned(s))
            .map_err(|e| anyhow!(e))
    }
}
