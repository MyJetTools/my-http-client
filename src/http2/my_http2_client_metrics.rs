pub trait MyHttp2ClientMetrics {
    fn instance_created(&self, name: &str);
    fn instance_disposed(&self, name: &str);
    fn connected(&self, name: &str);
    fn disconnected(&self, name: &str);
}
