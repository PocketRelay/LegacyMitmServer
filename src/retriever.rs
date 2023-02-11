//! Module for retrieving data from the official Mass Effect 3 Servers

use blaze_pk::{
    codec::{Decodable, Encodable},
    error::DecodeError,
    packet::{Packet, PacketComponents, PacketDebug, PacketType},
};
use blaze_ssl_async::stream::BlazeStream;
use log::{debug, error, log_enabled};
use serde::Deserialize;
use tokio::io::{self, AsyncWriteExt};

use crate::{
    components::{Components, Redirector},
    models::{InstanceDetails, InstanceRequest, Port},
};

/// Structure for the retrievier system which contains the host address
/// for the official game server in order to make further connections
pub struct Retriever {
    /// The host address of the official server
    host: String,
    /// The port of the official server.
    port: u16,
}

impl Retriever {
    /// The hostname for the redirector server
    const REDIRECTOR_HOST: &str = "gosredirector.ea.com";
    /// The port for the redirector server.
    const REDIRECT_PORT: Port = 42127;

    /// Attempts to create a new retriever by first retrieving the coorect
    /// ip address of the gosredirector.ea.com host and then creates a
    /// connection to the redirector server and obtains the IP and Port
    /// of the Official server.
    pub async fn new() -> Option<Retriever> {
        let redirector_host = lookup_host(Self::REDIRECTOR_HOST).await?;
        debug!("Completed host lookup: {}", &redirector_host);
        let (host, port) = Self::get_main_host(redirector_host).await?;
        debug!("Retriever setup complete. (Host: {} Port: {})", &host, port);
        Some(Retriever { host, port })
    }

    /// Makes a instance request to the redirect server at the provided
    /// host and returns the instance response.
    async fn get_main_host(host: String) -> Option<(String, Port)> {
        debug!("Connecting to official redirector");
        let stream = Self::stream_to(&host, Self::REDIRECT_PORT).await?;
        let mut session = RetSession::new(stream)?;
        debug!("Connected to official redirector");
        debug!("Requesting details from official server");
        let instance = session.get_main_instance().await.ok()?;
        let net = instance.net;
        Some((net.host.into(), net.port))
    }

    /// Returns a new stream to the mian server
    pub async fn stream_to(host: &String, port: Port) -> Option<BlazeStream> {
        let addr = (host.clone(), port);
        match BlazeStream::connect(addr).await {
            Ok(value) => Some(value),
            Err(err) => {
                error!(
                    "Failed to connect to server at {}:{}; Cause: {err:?}",
                    host, port
                );
                None
            }
        }
    }

    /// Returns a new stream to the main server
    pub async fn stream(&self) -> Option<BlazeStream> {
        Self::stream_to(&self.host, self.port).await
    }
}

/// Session implementation for a retriever client
pub struct RetSession {
    /// The ID for the next request packet
    id: u16,
    /// The underlying SSL / TCP stream connection
    stream: BlazeStream,
}

/// Error type for retriever errors
pub enum RetrieverError {
    /// Packet decode errror
    Decode(DecodeError),
    /// IO Error
    IO(io::Error),
    /// Error response packet
    Packet(Packet),
}

pub type RetrieverResult<T> = Result<T, RetrieverError>;

impl RetSession {
    /// Creates a new retriever session for the provided host and
    /// port. This will create the underlying connection aswell.
    /// If creating the connection fails then None is returned instead.
    pub fn new(stream: BlazeStream) -> Option<Self> {
        Some(Self { id: 0, stream })
    }

    /// Writes a request packet and waits until the response packet is
    /// recieved returning the contents of that response packet.
    pub async fn request<Req: Encodable, Res: Decodable>(
        &mut self,
        component: Components,
        contents: Req,
    ) -> RetrieverResult<Res> {
        let request = Packet::request(self.id, component, contents);
        request.write_async(&mut self.stream).await?;
        debug_log_packet(&request, "Sent to Official");
        self.stream.flush().await?;
        self.id += 1;
        let response = self.expect_response(&request).await?;
        let contents = response.decode::<Res>()?;
        Ok(contents)
    }

    /// Waits for a response packet to be recieved any notification packets
    /// that are recieved are handled in the handle_notify function.
    async fn expect_response(&mut self, request: &Packet) -> RetrieverResult<Packet> {
        loop {
            let response = Packet::read_async(&mut self.stream).await?;
            debug_log_packet(&response, "Received from Official");
            let header = &response.header;

            if let PacketType::Response = header.ty {
                if header.path_matches(&request.header) {
                    return Ok(response);
                }
            } else if let PacketType::Error = header.ty {
                return Err(RetrieverError::Packet(response));
            }
        }
    }

    /// Function for making the request for the official server instance
    /// from the redirector server.
    async fn get_main_instance(&mut self) -> RetrieverResult<InstanceDetails> {
        self.request::<InstanceRequest, InstanceDetails>(
            Components::Redirector(Redirector::GetServerInstance),
            InstanceRequest,
        )
        .await
    }
}

/// Logs the contents of the provided packet to the debug output along with
/// the header information.
///
/// `component` The component for the packet routing
/// `packet`    The packet that is being logged
/// `direction` The direction name for the packet
fn debug_log_packet(packet: &Packet, action: &str) {
    // Skip if debug logging is disabled
    if !log_enabled!(log::Level::Debug) {
        return;
    }
    let component = Components::from_header(&packet.header);
    let debug = PacketDebug {
        packet,
        component: &component,
        minified: false,
    };
    debug!("\n{}\n{:?}", action, debug);
}

impl From<DecodeError> for RetrieverError {
    fn from(err: DecodeError) -> Self {
        RetrieverError::Decode(err)
    }
}

impl From<io::Error> for RetrieverError {
    fn from(err: io::Error) -> Self {
        RetrieverError::IO(err)
    }
}

/// Structure for the lookup responses from the google DNS API
///
/// # Structure
///
/// ```
/// {
///   "Status": 0,
///   "TC": false,
///   "RD": true,
///   "RA": true,
///   "AD": false,
///   "CD": false,
///   "Question": [
///     {
///       "name": "gosredirector.ea.com.",
///       "type": 1
///     }
///   ],
///   "Answer": [
///     {
///       "name": "gosredirector.ea.com.",
///       "type": 1,
///       "TTL": 300,
///       "data": "159.153.64.175"
///     }
///   ],
///   "Comment": "Response from 2600:1403:a::43."
/// }
/// ```
#[derive(Deserialize)]
struct LookupResponse {
    #[serde(rename = "Answer")]
    answer: Vec<Answer>,
}

/// Structure for answer portion of request. Only the data value is
/// being used so only that is present here.
///
/// # Structure
/// ```
/// {
///   "name": "gosredirector.ea.com.",
///   "type": 1,
///   "TTL": 300,
///   "data": "159.153.64.175"
/// }
/// ```
#[derive(Deserialize)]
struct Answer {
    data: String,
}

/// Attempts to resolve the address value of the provided host. First attempts
/// to use the system DNS with tokio but if the resolved address is loopback it
/// is ignored and the google HTTP DNS will be attempted instead
///
/// `host` The host to lookup
async fn lookup_host(host: &str) -> Option<String> {
    // Attempt to lookup using the system DNS
    {
        let tokio = tokio::net::lookup_host(host)
            .await
            .ok()
            .and_then(|mut value| value.next());

        if let Some(tokio) = tokio {
            let ip = tokio.ip();
            // Loopback value means it was probbably redirected in the hosts file
            // so those are ignored
            if !ip.is_loopback() {
                return Some(format!("{}", ip));
            }
        }
    }

    // Attempt to lookup using google HTTP DNS
    let url = format!("https://dns.google/resolve?name={host}&type=A");
    let mut request = reqwest::get(url)
        .await
        .ok()?
        .json::<LookupResponse>()
        .await
        .ok()?;

    let answer = request.answer.pop()?;
    Some(answer.data)
}
