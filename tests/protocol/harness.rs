//! In-process server plus `wayland-client` test clients over socket pairs (research R13).
//!
//! Every event a test client receives is recorded with its object id, so tests can look for
//! typed events with [`Client::events`].

use std::any::Any;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

use rubickway::server::Server;
use wayland_client::backend::ObjectId;
use wayland_client::backend::protocol::ProtocolError;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

/// One received event.
pub struct Recorded {
    pub object: ObjectId,
    pub event: Box<dyn Any>,
}

/// What a test client keeps: the advertised globals and every event.
#[derive(Default)]
pub struct Recorder {
    pub globals: Vec<(u32, String, u32)>,
    pub events: Vec<Recorded>,
}

impl<I> Dispatch<I, ()> for Recorder
where
    I: Proxy + 'static,
    I::Event: 'static,
{
    fn event(
        state: &mut Self,
        proxy: &I,
        event: I::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let any: Box<dyn Any> = Box::new(event);
        match any.downcast_ref::<wl_registry::Event>() {
            Some(wl_registry::Event::Global {
                name,
                interface,
                version,
            }) => state.globals.push((*name, interface.clone(), *version)),
            Some(wl_registry::Event::GlobalRemove { name }) => {
                state.globals.retain(|(n, _, _)| n != name);
            }
            _ => {}
        }
        state.events.push(Recorded {
            object: proxy.id(),
            event: any,
        });
    }
}

/// A test client.
pub struct Client {
    pub conn: Connection,
    pub queue: EventQueue<Recorder>,
    pub qh: QueueHandle<Recorder>,
    pub state: Recorder,
    pub registry: WlRegistry,
    /// A paused client still sends but never reads its socket (misbehaving case 4).
    pub paused: bool,
}

impl Client {
    /// Binds the global named `interface` at `version`.
    pub fn bind<I: Proxy + 'static>(&self, version: u32) -> I
    where
        Recorder: Dispatch<I, ()>,
    {
        let interface = I::interface().name;
        let (name, _, _) = self
            .state
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .unwrap_or_else(|| panic!("no global {interface}"));
        self.registry.bind::<I, _, _>(*name, version, &self.qh, ())
    }

    /// Binds every advertised global named like `I`'s interface at `version`, in the order
    /// they were advertised.
    pub fn bind_all<I: Proxy + 'static>(&self, version: u32) -> Vec<I>
    where
        Recorder: Dispatch<I, ()>,
    {
        let interface = I::interface().name;
        self.state
            .globals
            .iter()
            .filter(|(_, i, _)| i == interface)
            .map(|(name, _, _)| self.registry.bind::<I, _, _>(*name, version, &self.qh, ()))
            .collect()
    }

    /// The events of type `E` received so far, with the object they arrived on.
    pub fn events<E: 'static>(&self) -> Vec<(ObjectId, &E)> {
        self.state
            .events
            .iter()
            .filter_map(|r| r.event.downcast_ref::<E>().map(|e| (r.object.clone(), e)))
            .collect()
    }

    /// Forgets the recorded events.
    pub fn clear_events(&mut self) {
        self.state.events.clear();
    }

    /// The protocol error that disconnected this client, if any.
    pub fn error(&self) -> Option<ProtocolError> {
        self.conn.protocol_error()
    }

    /// Whether the connection is gone (protocol error or closed by the server).
    pub fn is_disconnected(&self) -> bool {
        self.error().is_some() || self.conn.flush().is_err()
    }

    fn pump(&mut self) {
        let _ = self.conn.flush();
        if self.paused {
            return;
        }
        if let Some(guard) = self.conn.prepare_read() {
            let _ = guard.read();
        }
        let _ = self.queue.dispatch_pending(&mut self.state);
    }
}

/// A server with any number of connected test clients.
pub struct Harness {
    pub server: Server,
    pub clients: Vec<Client>,
}

impl Harness {
    pub fn new() -> Harness {
        Harness {
            server: Server::new().expect("server"),
            clients: Vec::new(),
        }
    }

    /// Connects a new client, waits for the globals, and returns its index.
    pub fn connect(&mut self) -> usize {
        let (server_end, client_end) = UnixStream::pair().expect("socket pair");
        client_end.set_nonblocking(true).expect("non-blocking");
        self.server
            .insert_client(server_end)
            .expect("insert client");
        let conn = Connection::from_socket(client_end).expect("client connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        let registry = conn.display().get_registry(&qh, ());
        self.clients.push(Client {
            conn,
            queue,
            qh,
            state: Recorder::default(),
            registry,
            paused: false,
        });
        self.roundtrip();
        self.clients.len() - 1
    }

    pub fn c(&mut self, index: usize) -> &mut Client {
        &mut self.clients[index]
    }

    /// Lets server and clients exchange messages until nothing is left in flight.
    pub fn roundtrip(&mut self) {
        for _ in 0..8 {
            for client in &mut self.clients {
                let _ = client.conn.flush();
            }
            self.server.dispatch().expect("server dispatch");
            self.server.flush().expect("server flush");
            for client in &mut self.clients {
                client.pump();
            }
        }
    }
}

/// An unlinked temporary file of `len` bytes.
pub fn temp_file(len: usize) -> File {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "rubickway-protocol-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("temp file");
    std::fs::remove_file(&path).expect("unlink temp file");
    file.set_len(len as u64).expect("size temp file");
    file
}

/// An shm pool of `width x height` pixels filled with `argb`, and a buffer covering it. The
/// file is returned so tests can misuse it.
pub fn solid_buffer(
    client: &Client,
    shm: &WlShm,
    width: i32,
    height: i32,
    argb: u32,
) -> (WlBuffer, WlShmPool, File) {
    let stride = width * 4;
    let len = (stride * height) as usize;
    let mut file = temp_file(len);
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|_| argb.to_le_bytes())
        .collect();
    file.write_all(&pixels).expect("fill buffer");
    let pool = shm.create_pool(file.as_fd(), len as i32, &client.qh, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        stride,
        wl_shm::Format::Argb8888,
        &client.qh,
        (),
    );
    (buffer, pool, file)
}
