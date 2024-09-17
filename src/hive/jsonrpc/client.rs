use std::time::Duration;
use color_eyre::Report;
use jsonrpsee::core::client::Error;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee_http_client::transport::HttpBackend;
use tower_http::compression::Compression;
use tower_http::decompression::Decompression;
use tracing::info;

pub(crate) trait JsonRpcClient {
    fn new(rpc_nodes: Vec<String>) -> Result<Self, Report> where Self: Sized;
    fn build_client(first_rpc_node: &String) -> Result<HttpClient<Decompression<Compression<HttpBackend>>>, Error>;
    fn get_client(&self) -> &HttpClient<Decompression<Compression<HttpBackend>>>;
    fn rotate_node(&mut self) -> Result<(), Report>;
}

pub(crate) struct JsonRpcClientImpl {
    client: HttpClient<Decompression<Compression<HttpBackend>>>,
    rpc_nodes: Vec<String>,
    current_node: usize
}

impl JsonRpcClient for JsonRpcClientImpl {
    fn new(rpc_nodes: Vec<String>) -> Result<JsonRpcClientImpl, Report> {
        let first_rpc_node = rpc_nodes.get(0).expect("No RPC Nodes defined!").clone();

        info!("Using first RPC Node: {}", first_rpc_node);

        Ok(JsonRpcClientImpl {
            rpc_nodes,
            current_node: 0,
            client: Self::build_client(&first_rpc_node)?
        })
    }

    fn build_client(first_rpc_node: &String) -> Result<HttpClient<Decompression<Compression<HttpBackend>>>, Error> {
        let middleware_stack = tower::ServiceBuilder::new()
            .layer(
                tower_http::decompression::DecompressionLayer::new()
                    .gzip(true).deflate(true).br(true).zstd(true),
            )
            .layer(
                tower_http::compression::CompressionLayer::new()
                    .gzip(true).deflate(true).br(true).zstd(true),
            );

        HttpClient::builder()
            .max_request_size(50 * 1024 * 1024)
            .max_response_size(50 * 1024 * 1024)
            .request_timeout(Duration::from_secs(30))
            .set_http_middleware(middleware_stack)
            .build(first_rpc_node)
    }

    fn get_client(&self) -> &HttpClient<Decompression<Compression<HttpBackend>>> {
        &self.client
    }

    fn rotate_node(&mut self) -> Result<(), Report> {
        self.current_node = if self.current_node + 1 < self.rpc_nodes.len() {
            self.current_node + 1
        } else {
            0
        };

        let next_node = self.rpc_nodes.get(self.current_node).unwrap();

        info!("Using next RPC Node: {}", next_node);

        self.client = Self::build_client(next_node)?;

        Ok(())
    }
}
