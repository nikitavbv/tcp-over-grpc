use {
    std::{pin::Pin, collections::HashMap},
    tonic::{
        transport::{Server, Channel, ClientTlsConfig},
        Status,
        Request,
        Streaming,
        Response,
        metadata::{AsciiMetadataValue, AsciiMetadataKey},
    },
    tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream},
    tokio::{net::{TcpStream, TcpListener}, io::{AsyncWriteExt, AsyncReadExt}, sync::mpsc},
    clap::{Parser, Subcommand},
    crate::rpc::{
        tcp_over_grpc_service_server::{TcpOverGrpcService, TcpOverGrpcServiceServer},
        tcp_over_grpc_service_client::TcpOverGrpcServiceClient,
        IngressMessage,
        EgressMessage,
    },
};

mod rpc {
    tonic::include_proto!("tcp_over_grpc");
}

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    #[clap(subcommand)]
    mode: CommandMode,
}

#[derive(Debug, Subcommand)]
enum CommandMode {
    /// Spin up entry node. Receives incoming TCP and forwards GRPC.
    Entry {
        #[clap(short, long, default_value = "localhost:1415")]
        bind_addr: String,

        /// URL of the exit node
        #[clap(short, long, value_parser)]
        target_url: String,

        #[clap(short, long, value_parser)]
        headers: Vec<String>,
    },
    /// Spin up exit node. Receives incoming GRPC and forwards TCP.
    Exit {
        #[clap(short, long, default_value = "localhost:8080")]
        bind_addr: String,

        #[clap(short, long)]
        target_addr: String,
    }
}

#[tokio::main]
async fn main() {
    match Args::parse().mode {
        CommandMode::Exit { bind_addr, target_addr } => run_server(bind_addr, target_addr).await,
        CommandMode::Entry {
            bind_addr,
            target_url,
            headers ,
        } => run_client(
            bind_addr,
            target_url,
            headers
                .into_iter()
                .map(|header| {
                    let colon_index = header.find(':').unwrap();
                    let (k, v) = header.split_at(colon_index);
                    let k = k.trim().to_owned().to_lowercase();
                    let v = v[1..].trim().to_owned();
                    (k, v)
                })
                .collect::<HashMap<String, String>>()
        ).await,
    }
}

async fn run_client(bind_addr: String, target_url: String, headers: HashMap<String, String>) {
    println!("running tcp-over-grpc client on {}", bind_addr);

    let listener = TcpListener::bind(bind_addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let (mut tcp_read, mut tcp_write) = socket.into_split();

        let endpoint = Channel::from_shared(target_url.clone()).unwrap()
            .tls_config(ClientTlsConfig::new().with_enabled_roots())
            .unwrap();
        let mut client = TcpOverGrpcServiceClient::connect(endpoint).await.unwrap();

        let (tx, rx) = mpsc::channel(128);
        let in_stream = ReceiverStream::new(rx);

        let mut req = Request::new(in_stream);
        for (k, v) in &headers {
            req.metadata_mut()
                .append(
                    AsciiMetadataKey::from_bytes(k.as_bytes()).unwrap(),
                    AsciiMetadataValue::try_from(v).unwrap()
                );
        }

        let mut res = client.wrap_tcp_stream(req).await.unwrap().into_inner();

        tokio::spawn(async move {
            let mut buf = vec![0; 1024];

            loop {
                let n = tcp_read
                    .read(&mut buf)
                    .await
                    .unwrap();

                if n == 0 {
                    return;
                }

                tx.send(IngressMessage {
                    data: buf[0..n].to_vec(),
                }).await.unwrap();
            }
        });

        tokio::spawn(async move {
            while let Some(received) = res.next().await {
                let received = received.unwrap();
                tcp_write.write_all(&received.data).await.unwrap();
            }
        });
    }
}

async fn run_server(bind_addr: String, target_addr: String) {
    println!("running tcp-over-grpc server on {}", bind_addr);

    Server::builder()
        .add_service(TcpOverGrpcServiceServer::new(TcpOverGrpcHandler::new(target_addr)))
        .serve(bind_addr.parse().unwrap())
        .await
        .unwrap();
}

struct TcpOverGrpcHandler {
    target_addr: String,
}

impl TcpOverGrpcHandler {
    pub fn new(target_addr: String) -> Self {
        Self {
            target_addr,
        }
    }
}

#[tonic::async_trait]
impl TcpOverGrpcService for TcpOverGrpcHandler {
    type WrapTcpStreamStream = Pin<Box<dyn Stream<Item = Result<EgressMessage, Status>> + Send>>;

    async fn wrap_tcp_stream(&self, req: Request<Streaming<IngressMessage>>) -> Result<Response<Self::WrapTcpStreamStream>, Status> {
        let mut req_stream = req.into_inner();
        let target_stream = TcpStream::connect(&self.target_addr).await.unwrap();
        let (mut rx, mut tx) = target_stream.into_split();
        let (res_tx, res_rx) = mpsc::channel(128);

        tokio::spawn(async move {
            while let Some(result) = req_stream.next().await {
                let v = result.unwrap();
                tx.write_all(&v.data).await.unwrap();
            }
        });

        tokio::spawn(async move {
            let mut buf = vec![0; 1024];

            loop {
                let n = rx
                    .read(&mut buf)
                    .await
                    .unwrap();

                if n == 0 {
                    return;
                }

                res_tx.send(Ok(EgressMessage {
                    data: buf[0..n].to_vec(),
                })).await.unwrap();
            }
        });

        let out_stream = ReceiverStream::new(res_rx);

        Ok(Response::new(
            Box::pin(out_stream) as Self::WrapTcpStreamStream
        ))
    }
}
