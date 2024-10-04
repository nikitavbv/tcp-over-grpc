use {
    std::pin::Pin,
    tonic::{transport::Server, Status, Request, Streaming, Response},
    tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream},
    tokio::{net::TcpStream, io::{AsyncWriteExt, AsyncReadExt}, sync::mpsc},
    crate::rpc::{
        tcp_over_grpc_service_server::{TcpOverGrpcService, TcpOverGrpcServiceServer},
        IngressMessage,
        EgressMessage,
    },
};

mod rpc {
    tonic::include_proto!("tcp_over_grpc");
}

#[tokio::main]
async fn main() {
    println!("tcp-over-grpc is running");

    Server::builder()
        .add_service(TcpOverGrpcServiceServer::new(TcpOverGrpcHandler))
        .serve("0.0.0.0:8080".parse().unwrap())
        .await
        .unwrap();
}

struct TcpOverGrpcHandler;

impl TcpOverGrpcHandler {
}

#[tonic::async_trait]
impl TcpOverGrpcService for TcpOverGrpcHandler {
    type WrapTcpStreamStream = Pin<Box<dyn Stream<Item = Result<EgressMessage, Status>> + Send>>;

    async fn wrap_tcp_stream(&self, req: Request<Streaming<IngressMessage>>) -> Result<Response<Self::WrapTcpStreamStream>, Status> {
        let mut req_stream = req.into_inner();
        let target_stream = TcpStream::connect("tcpbin.com:4242").await.unwrap();
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
