// Хранение ключа сессии в системном хранилище секретов:
//   Windows — Диспетчер учётных данных (Credential Manager), «Учётные данные Windows»;
//   macOS   — Связка ключей (Keychain).
// В файлы, настройки и логи ключ не пишется никогда.

use super::key::SessionKey;

/// Имя «сервиса» в хранилище. В Диспетчере учётных данных видно как
/// `tokencat-session.app.tokencat.desktop` (так keyring формирует имя).
/// Учётка у каждого профиля своя: `tokencat-session`, `tokencat-session-2`… (profile.rs).
pub const SERVICE: &str = "app.tokencat.desktop";

pub trait SecretStore: Send + Sync {
    /// Ok(None) — ключа нет (не входили или вышли).
    fn load(&self) -> Result<Option<SessionKey>, String>;
    fn save(&self, key: &SessionKey) -> Result<(), String>;
    /// Удалить ключ. Отсутствие ключа — не ошибка.
    fn delete(&self) -> Result<(), String>;
}

/// Настоящее системное хранилище (crate `keyring`), одна учётка = один профиль.
pub struct KeyringStore {
    account: String,
}

impl KeyringStore {
    pub fn new(account: impl Into<String>) -> Self {
        KeyringStore { account: account.into() }
    }

    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &self.account).map_err(|e| format!("Хранилище секретов: {e}"))
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<SessionKey>, String> {
        match self.entry()?.get_password() {
            Ok(raw) => match SessionKey::from_trusted(&raw) {
                Ok(k) => Ok(Some(k)),
                // Испорченная запись — считаем, что ключа нет, и убираем её
                Err(_) => {
                    let _ = self.delete();
                    Ok(None)
                }
            },
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Не удалось прочитать ключ из хранилища: {e}")),
        }
    }

    fn save(&self, key: &SessionKey) -> Result<(), String> {
        self.entry()?
            .set_password(key.expose())
            .map_err(|e| format!("Не удалось сохранить ключ в хранилище: {e}"))
    }

    fn delete(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Не удалось удалить ключ из хранилища: {e}")),
        }
    }
}

/// Хранилище в памяти — для тестов.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(pub std::sync::Mutex<Option<String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<SessionKey>, String> {
        let g = self.0.lock().unwrap();
        Ok(g.as_deref().and_then(|s| SessionKey::from_trusted(s).ok()))
    }
    fn save(&self, key: &SessionKey) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(key.expose().to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "sk-ant-sid01-AbCdEfGhIjKlMnOpQrStUvWxYz_0123456789";

    #[test]
    fn save_load_delete_roundtrip() {
        let s = MemoryStore::default();
        assert!(s.load().unwrap().is_none());
        let k = SessionKey::from_trusted(GOOD).unwrap();
        s.save(&k).unwrap();
        assert_eq!(s.load().unwrap().unwrap(), k);
        s.delete().unwrap();
        assert!(s.load().unwrap().is_none());
        // Повторное удаление — не ошибка
        s.delete().unwrap();
    }

    #[test]
    fn broken_record_is_ignored() {
        let s = MemoryStore(std::sync::Mutex::new(Some("мусор".into())));
        assert!(s.load().unwrap().is_none());
    }

    /// Настоящее хранилище ОС. Запускается вручную: `cargo test -- --ignored`
    /// (трогает Credential Manager / Keychain, поэтому не в обычном прогоне).
    #[test]
    #[ignore]
    fn real_keyring_roundtrip() {
        let store = KeyringStore::new(crate::profile::BASE_ACCOUNT);
        let backup = store.load().unwrap();
        let k = SessionKey::from_trusted(GOOD).unwrap();
        store.save(&k).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), k);
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
        if let Some(b) = backup {
            store.save(&b).unwrap();
        }
    }
}
