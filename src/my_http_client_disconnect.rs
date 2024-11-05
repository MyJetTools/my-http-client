pub trait MyHttpClientDisconnect {
    fn disconnect(&self);
    fn get_connection_id(&self) -> u64;
}
