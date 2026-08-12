use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::json;

use crate::{metrics::SystemMetrics, model::DashboardData};

pub struct DynaCatPublisher {
    routes: Arc<RwLock<HashMap<String, String>>>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl DynaCatPublisher {
    pub fn start(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "DynaCat-Socket muss ein absoluter Pfad sein",
            ));
        }
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Socket hat kein Elternverzeichnis",
            )
        })?;
        if !parent.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Elternverzeichnis des DynaCat-Sockets fehlt",
            ));
        }
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if !metadata.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Vorhandener DynaCat-Pfad ist kein Unix-Socket",
                ));
            }
            fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
        listener.set_nonblocking(true)?;
        let routes = Arc::new(RwLock::new(initial_routes()));
        let worker_routes = Arc::clone(&routes);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let worker = thread::spawn(move || serve(listener, worker_routes, worker_shutdown));
        Ok(Self {
            routes,
            shutdown,
            worker: Some(worker),
            path: path.to_path_buf(),
        })
    }

    pub fn publish(
        &self,
        data: &DashboardData,
        metrics: &SystemMetrics,
        job: Option<(&str, &str, bool, Option<bool>)>,
    ) {
        let next_routes = build_routes(data, metrics, job);
        let mut routes = match self.routes.write() {
            Ok(routes) => routes,
            Err(poisoned) => poisoned.into_inner(),
        };
        *routes = next_routes;
    }
}

fn build_routes(
    data: &DashboardData,
    metrics: &SystemMetrics,
    job: Option<(&str, &str, bool, Option<bool>)>,
) -> HashMap<String, String> {
    let public_players = data
        .players
        .iter()
        .map(|player| {
            json!({
                "name": player.name,
                "level": player.level,
                "ping": player.ping,
                "building_count": player.building_count,
            })
        })
        .collect::<Vec<_>>();
    let public_settings = data
        .settings
        .iter()
        .filter(|setting| !setting.secret)
        .map(|setting| {
            json!({
                "key": setting.key,
                "label": setting.label,
                "category": setting.category,
                "value": setting.value,
                "editable": setting.is_editable(),
            })
        })
        .collect::<Vec<_>>();
    let mut routes = HashMap::new();
    routes.insert(
        "/v1/health".to_owned(),
        json!({
            "status": if data.server.api_connected { "ok" } else { "degraded" },
            "service_active": data.server.service_active,
            "palworld_api_connected": data.server.api_connected,
        })
        .to_string(),
    );
    routes.insert(
        "/v1/status".to_owned(),
        serde_json::to_string(&data.server).unwrap_or_else(|_| "{}".to_owned()),
    );
    routes.insert(
        "/v1/resources".to_owned(),
        json!({
            "cpu_percent": metrics.cpu_percent,
            "memory_percent": metrics.memory_percent,
            "memory_used_gib": metrics.memory_used_gib,
            "memory_total_gib": metrics.memory_total_gib,
            "disk_percent": metrics.disk_percent,
            "disk_free_gib": metrics.disk_free_gib,
        })
        .to_string(),
    );
    routes.insert("/v1/players".to_owned(), json!(public_players).to_string());
    routes.insert(
        "/v1/settings".to_owned(),
        json!(public_settings).to_string(),
    );
    routes.insert(
        "/v1/mods".to_owned(),
        serde_json::to_string(&data.mods).unwrap_or_else(|_| "[]".to_owned()),
    );
    routes.insert(
        "/v1/backups".to_owned(),
        serde_json::to_string(&data.backups).unwrap_or_else(|_| "[]".to_owned()),
    );
    routes.insert(
        "/v1/events".to_owned(),
        json!(data.logs.iter().rev().take(50).collect::<Vec<_>>()).to_string(),
    );
    routes.insert(
        "/v1/jobs".to_owned(),
        job.map_or_else(
            || json!({ "active": false }).to_string(),
            |(label, detail, running, success)| {
                json!({
                    "active": running,
                    "label": label,
                    "detail": detail,
                    "success": success,
                })
                .to_string()
            },
        ),
    );
    routes
}

impl Drop for DynaCatPublisher {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn serve(
    listener: UnixListener,
    routes: Arc<RwLock<HashMap<String, String>>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => handle_client(stream, &routes),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

fn handle_client(mut stream: UnixStream, routes: &RwLock<HashMap<String, String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut buffer = [0_u8; 8192];
    let Ok(length) = stream.read(&mut buffer) else {
        return;
    };
    let request = String::from_utf8_lossy(&buffer[..length]);
    let Some(first_line) = request.lines().next() else {
        return;
    };
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let route = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    let (status, body) = if method != "GET" {
        (
            "405 Method Not Allowed",
            r#"{"error":"read-only API"}"#.to_owned(),
        )
    } else {
        let routes = match routes.read() {
            Ok(routes) => routes,
            Err(poisoned) => poisoned.into_inner(),
        };
        routes.get(route).cloned().map_or_else(
            || ("404 Not Found", r#"{"error":"not found"}"#.to_owned()),
            |body| ("200 OK", body),
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn initial_routes() -> HashMap<String, String> {
    HashMap::from([(
        "/v1/health".to_owned(),
        r#"{"status":"starting"}"#.to_owned(),
    )])
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        sync::{Arc, RwLock},
        thread,
    };

    use crate::{backend::demo_data, metrics::SystemMetrics};

    use super::{build_routes, handle_client, initial_routes};

    #[test]
    fn api_starts_with_health_route_only() {
        let routes = initial_routes();
        assert!(routes.contains_key("/v1/health"));
        assert!(!routes.contains_key("/v1/write"));
    }

    #[test]
    fn socket_api_omits_private_player_identifiers() -> std::io::Result<()> {
        let mut metrics = SystemMetrics::new(true);
        metrics.refresh();
        let mut data = demo_data();
        data.players[0].account_name = "internal-account-marker".to_owned();
        data.players[0].player_id = "internal-player-marker".to_owned();
        data.players[0].user_id = "internal-user-marker".to_owned();
        let routes = Arc::new(RwLock::new(build_routes(&data, &metrics, None)));
        let (mut client, server) = UnixStream::pair()?;
        let server_routes = Arc::clone(&routes);
        let worker = thread::spawn(move || handle_client(server, &server_routes));

        client.write_all(b"GET /v1/players HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        client.read_to_string(&mut response)?;
        assert!(worker.join().is_ok());
        assert!(response.contains("Demo Player 1"));
        assert!(!response.contains("internal-account-marker"));
        assert!(!response.contains("internal-player-marker"));
        assert!(!response.contains("internal-user-marker"));

        Ok(())
    }
}
