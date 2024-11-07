pub trait MyHttp2ClientMetrics {
    fn connected(&self, name: &str);
    fn disconnected(&self, name: &str);
}
