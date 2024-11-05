pub trait MyHttpClientMetrics {
    fn tcp_connect(&self, name: &str);
    fn tcp_disconnect(&self, name: &str);
    fn read_thread_start(&self, name: &str);
    fn read_thread_stop(&self, name: &str);
    fn write_thread_start(&self, name: &str);
    fn write_thread_stop(&self, name: &str);
}
