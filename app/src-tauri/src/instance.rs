// Один экземпляр на профиль (B1, п. 1).
//
// Плагин tauri-plugin-single-instance привязан к идентификатору приложения, то есть
// разрешает одну копию на всю программу, а нам нужна одна копия на профиль (несколько
// аккаунтов, п. 9). Поэтому своё, простое и одинаковое на Windows и macOS:
//   - первая копия слушает случайный порт на 127.0.0.1 и пишет его в `instance.port`
//     в папке профиля;
//   - новая копия читает порт, стучится туда строкой `TOKENCAT-SHOW <профиль>` и, если
//     получила `OK`, закрывается, а первая показывает свою панель;
//   - не достучалась (порт устарел, программа упала) — значит, она первая.
// Наружу порт не виден (только 127.0.0.1), а чужая строка просто игнорируется.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE: &str = "instance.port";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const IO_TIMEOUT: Duration = Duration::from_millis(1500);

pub enum Start {
    /// Мы первые. Listener может быть None, если порт открыть не удалось (тогда просто
    /// работаем без защиты от второй копии).
    First(Option<TcpListener>),
    /// Этот профиль уже открыт — он покажет себя сам, нам нужно выйти.
    AlreadyRunning,
}

fn hello(profile_id: &str) -> String {
    format!("TOKENCAT-SHOW {profile_id}")
}

fn port_file(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Постучаться в уже открытую копию. true — она ответила и покажет себя.
fn knock(dir: &Path, profile_id: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(port_file(dir)) else { return false };
    let Ok(port) = text.trim().parse::<u16>() else { return false };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else { return false };
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    if writeln!(s, "{}", hello(profile_id)).is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).is_ok() && line.trim() == "OK"
}

/// Вызывать в самом начале запуска профиля.
pub fn acquire(dir: &Path, profile_id: &str) -> Start {
    if knock(dir, profile_id) {
        return Start::AlreadyRunning;
    }
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
        return Start::First(None);
    };
    let Ok(addr) = listener.local_addr() else { return Start::First(None) };
    let _ = std::fs::create_dir_all(dir);
    if std::fs::write(port_file(dir), addr.port().to_string()).is_err() {
        return Start::First(None);
    }
    Start::First(Some(listener))
}

/// Слушать «стуки» новых копий в отдельном потоке. `on_show` вызывается на каждый
/// правильный стук.
pub fn listen(listener: TcpListener, profile_id: String, on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let expected = hello(&profile_id);
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.set_read_timeout(Some(IO_TIMEOUT));
            let _ = s.set_write_timeout(Some(IO_TIMEOUT));
            let mut line = String::new();
            // Больше 256 байт от незнакомца не читаем
            let ok = match s.try_clone() {
                Ok(r) => BufReader::new(Read::take(r, 256)).read_line(&mut line).is_ok(),
                Err(_) => false,
            };
            if ok && line.trim() == expected {
                on_show();
                let _ = writeln!(s, "OK");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn second_copy_knocks_first() {
        let dir = std::env::temp_dir().join(format!("tokencat-test-instance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let Start::First(Some(l)) = acquire(&dir, "2") else { panic!("первая копия") };
        let shown = Arc::new(AtomicU32::new(0));
        let s2 = shown.clone();
        listen(l, "2".into(), move || {
            s2.fetch_add(1, Ordering::SeqCst);
        });

        // Вторая копия того же профиля — видит первую
        assert!(matches!(acquire(&dir, "2"), Start::AlreadyRunning));
        assert_eq!(shown.load(Ordering::SeqCst), 1);

        // Чужой профиль с тем же файлом порта не считается (на всякий случай)
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::copy(dir.join(FILE), other.join(FILE)).unwrap();
        assert!(matches!(acquire(&other, "3"), Start::First(_)));
        assert_eq!(shown.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_port_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("tokencat-test-instance2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "1").unwrap(); // никто не слушает
        assert!(matches!(acquire(&dir, "default"), Start::First(Some(_))));
        std::fs::write(dir.join(FILE), "мусор").unwrap();
        assert!(matches!(acquire(&dir, "default"), Start::First(Some(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
