use cvmfs_server_scraper::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{Notify, Semaphore},
    task::{JoinHandle, JoinSet},
};

pub fn manifest(name: &str) -> Vec<u8> {
    format!(
        "C{}\nRd41d8cd98f00b204e9800998ecf8427e\nD900\nS42\nN{name}\n",
        "ab".repeat(20)
    )
    .into_bytes()
}
pub fn index(names: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"schema": 1, "replicas": names.iter().map(|name| serde_json::json!({"name":name,"url":format!("/cvmfs/{name}")})).collect::<Vec<_>>(), "repositories": []})).unwrap()
}
pub struct Reply {
    pub bytes: Vec<u8>,
    pub gate: Option<Arc<Semaphore>>,
    pub delay: Option<Duration>,
}
impl Reply {
    pub fn status(status: u16, body: impl AsRef<[u8]>) -> Self {
        let body = body.as_ref();
        let mut bytes = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        Self::raw(bytes)
    }
    pub fn ok(body: impl AsRef<[u8]>) -> Self {
        Self::status(200, body)
    }
    pub fn raw(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            gate: None,
            delay: None,
        }
    }
    pub fn gated(mut self, gate: Arc<Semaphore>) -> Self {
        self.gate = Some(gate);
        self
    }
    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
    pub fn redirect(location: &str) -> Self {
        Self::raw(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes())
    }
}
pub fn normal_reply(path: &str) -> Reply {
    if path.ends_with("repositories.json") {
        return Reply::ok(index(&["example.org"]));
    }
    if path.ends_with("meta.json") {
        return Reply::status(404, "");
    }
    if path.ends_with(".cvmfs_status.json") {
        return Reply::ok("{}");
    }
    if path.ends_with(".cvmfspublished") {
        return Reply::ok(manifest(path.split('/').nth(2).unwrap()));
    }
    if path.contains("/geo/") {
        return Reply::ok("1,2,3\n");
    }
    Reply::status(404, "")
}
#[derive(Default)]
pub struct Requests {
    paths: Mutex<Vec<String>>,
    changed: Notify,
    active: AtomicUsize,
    peak: AtomicUsize,
}
impl Requests {
    pub fn paths(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
    pub async fn wait_for(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let changed = self.changed.notified();
                if self.paths.lock().unwrap().len() >= count {
                    break;
                }
                changed.await;
            }
        })
        .await
        .expect("expected requests did not arrive");
    }
}
struct Active(Arc<Requests>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
pub struct Fixture {
    pub endpoint: ServerEndpoint,
    pub requests: Arc<Requests>,
    task: JoinHandle<()>,
}
impl Fixture {
    pub async fn new(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let requests = Arc::new(Requests::default());
        let tracker = requests.clone();
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            let mut workers = JoinSet::new();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (mut socket, _) = result.unwrap();
                        let requests = tracker.clone();
                        let handler = handler.clone();
                        workers.spawn(async move {
                            let mut data = Vec::new();
                            let mut buffer = [0; 1024];
                            while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                                let size = socket.read(&mut buffer).await.unwrap();
                                if size == 0 { return; }
                                data.extend_from_slice(&buffer[..size]);
                                assert!(data.len() <= 64 * 1024);
                            }
                            let text = String::from_utf8(data).unwrap();
                            let path = text.split_whitespace().nth(1).unwrap();
                            let active = requests.active.fetch_add(1, Ordering::SeqCst) + 1;
                            requests.peak.fetch_max(active, Ordering::SeqCst);
                            let _active = Active(requests.clone());
                            requests.paths.lock().unwrap().push(path.to_owned());
                            requests.changed.notify_waiters();
                            let reply = handler(path);
                            if let Some(gate) = reply.gate { gate.acquire().await.unwrap().forget(); }
                            if let Some(delay) = reply.delay { tokio::time::sleep(delay).await; }
                            let _ = socket.write_all(&reply.bytes).await;
                        });
                    }
                    result = workers.join_next(), if !workers.is_empty() => { result.unwrap().unwrap(); }
                }
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }
    pub fn server(&self, backend: ServerBackendType) -> Server {
        Server::new(ServerType::Stratum1, backend, self.endpoint.clone())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub fn only(names: &[&str]) -> ScrapeOptions {
    ScrapeOptions::default()
        .with_selection(RepositorySelection::only(
            names.iter().map(|name| name.parse().unwrap()),
        ))
        .with_geoapi(GeoapiProbe::Disabled)
}
