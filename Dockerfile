FROM ubuntu:22.04

COPY target/release/tcp-over-grpc /usr/local/bin/tcp-over-grpc

ENTRYPOINT ["/usr/local/bin/tcp-over-grpc"]
