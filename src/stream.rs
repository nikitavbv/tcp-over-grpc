use {
    std::{collections::HashMap, future::Future, task::Poll, pin::Pin, io},
    tokio::{io::{AsyncRead, AsyncWrite}, sync::mpsc::{self, Sender}},
    tokio_stream::{wrappers::ReceiverStream, Stream},
    tonic::{
        metadata::{AsciiMetadataKey, AsciiMetadataValue},
        transport::{Channel, ClientTlsConfig}, Request, Streaming
    },
    crate::rpc::{
        tcp_over_grpc_service_client::TcpOverGrpcServiceClient,
        EgressMessage,
        IngressMessage,
    },
};

#[pin_project::pin_project]
pub struct TcpOverGrpcStream {
    #[pin]
    tx_stream: Sender<IngressMessage>,
    #[pin]
    rx_stream: Streaming<EgressMessage>,

    read_buffer: Vec<u8>,
}

impl TcpOverGrpcStream {
    pub async fn new(endpoint: String, headers: HashMap<String, String>) -> Self {
        let endpoint = Channel::from_shared(endpoint).unwrap()
            .tls_config(ClientTlsConfig::new().with_enabled_roots())
            .unwrap();
        let mut client = TcpOverGrpcServiceClient::connect(endpoint).await.unwrap();

        let (tx_stream, rx) = mpsc::channel(128);
        let in_stream = ReceiverStream::new(rx);
        let mut req = Request::new(in_stream);
        for (k, v) in &headers {
            req.metadata_mut()
                .append(
                    AsciiMetadataKey::from_bytes(k.as_bytes()).unwrap(),
                    AsciiMetadataValue::try_from(v).unwrap()
                );
        }

        let rx_stream = client.wrap_tcp_stream(req).await.unwrap().into_inner();
        Self {
            tx_stream,
            rx_stream,

            read_buffer: Vec::new(),
        }
    }
}

impl AsyncRead for TcpOverGrpcStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_buffer.len() > 0 {
            let slice_to_put = buf.remaining().min(self.read_buffer.len());

            buf.put_slice(&self.read_buffer[0..slice_to_put]);
            self.read_buffer.drain(0..slice_to_put);
            return Poll::Ready(Ok(()));
        }

        let project = self.as_mut().project();
        let stream_next = project.rx_stream.poll_next(cx);
        match stream_next {
            Poll::Ready(v) => {
                if let Some(v) = v {
                    let v = v.unwrap();
                    let slice_to_put = buf.remaining().min(v.data.len());

                    buf.put_slice(&v.data[0..slice_to_put]);
                    if slice_to_put < v.data.len() {
                        self.read_buffer.append(&mut v.data[slice_to_put..].to_vec());
                    }
                }
                Poll::Ready(Ok(()))
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for TcpOverGrpcStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let project = self.project();
        let mut future = std::pin::pin!(project.tx_stream.send(IngressMessage {
            data: buf.to_vec(),
        }));

        loop {
            match future.as_mut().poll(cx) {
                Poll::Ready(_) => return Poll::Ready(Ok(buf.len())),
                Poll::Pending => continue,
            }
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        let buf = bufs.into_iter().flat_map(|v| v.into_iter().map(|v| *v)).collect::<Vec<_>>();
        self.poll_write(cx, &buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}
