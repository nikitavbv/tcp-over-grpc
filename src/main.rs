use {
    std::pin::Pin,
    tonic::{transport::Server, Status, Request, Streaming, Response},
    tokio_stream::{Stream, StreamExt},
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
        unimplemented!()
    }
}
