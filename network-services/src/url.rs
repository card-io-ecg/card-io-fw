use core::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Scheme {
    Http,
    Https,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BaseUrl<'a> {
    pub scheme: Scheme,
    /// As written: the `Host` header and the TLS server name.
    pub host: &'a str,
    /// The `Host` header carries `:port` only when `Some`.
    pub explicit_port: Option<u16>,
    /// `""` or `/…` without a trailing `/`.
    pub path: &'a str,
}

impl BaseUrl<'_> {
    pub fn port(&self) -> u16 {
        if let Some(port) = self.explicit_port {
            return port;
        }
        match self.scheme {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }

    pub fn ipv4(&self) -> Option<Ipv4Addr> {
        self.host.parse().ok()
    }
}

pub fn parse(text: &str) -> Option<BaseUrl<'_>> {
    if text.is_empty() || text.bytes().any(forbidden) {
        return None;
    }
    let (scheme, rest) = scheme(text)?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let (host, explicit_port) = authority_parts(authority)?;
    if !valid_host(host) {
        return None;
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    Some(BaseUrl {
        scheme,
        host,
        explicit_port,
        path,
    })
}

fn forbidden(byte: u8) -> bool {
    matches!(byte, b'@' | b' ' | b'?' | b'#' | b'"' | b'\\') || byte < 0x20 || byte == 0x7f
}

fn scheme(text: &str) -> Option<(Scheme, &str)> {
    if let Some(rest) = strip_prefix_ignore_ascii_case(text, b"https://") {
        return Some((Scheme::Https, rest));
    }
    if let Some(rest) = strip_prefix_ignore_ascii_case(text, b"http://") {
        return Some((Scheme::Http, rest));
    }
    None
}

fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &[u8]) -> Option<&'a str> {
    let bytes = text.as_bytes();
    if bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&text[prefix.len()..])
    } else {
        None
    }
}

fn authority_parts(authority: &str) -> Option<(&str, Option<u16>)> {
    let Some((host, port_text)) = authority.rsplit_once(':') else {
        return Some((authority, None));
    };
    if host.is_empty() {
        return None;
    }
    Some((host, Some(port(port_text)?)))
}

fn port(text: &str) -> Option<u16> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port: u32 = text.parse().ok()?;
    u16::try_from(port).ok().filter(|port| *port != 0)
}

fn valid_host(host: &str) -> bool {
    let bytes = host.as_bytes();
    if bytes.is_empty() || bytes.len() > 253 {
        return false;
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.'))
    {
        return false;
    }
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b'.')
    {
        return host.parse::<Ipv4Addr>().is_ok();
    }
    true
}

#[cfg(test)]
mod test {
    use std::format;

    use super::*;

    fn url(text: &str) -> BaseUrl<'_> {
        parse(text).unwrap_or_else(|| panic!("rejected {text}"))
    }

    #[test]
    fn https_base_url_with_a_path() {
        let parsed = url("https://backend.example.com/api");
        assert_eq!(parsed.scheme, Scheme::Https);
        assert_eq!(parsed.host, "backend.example.com");
        assert_eq!(parsed.explicit_port, None);
        assert_eq!(parsed.port(), 443);
        assert_eq!(parsed.path, "/api");
        assert_eq!(parsed.ipv4(), None);
    }

    #[test]
    fn ipv4_base_url_drops_one_trailing_slash() {
        let parsed = url("http://192.168.0.196:3001/api/");
        assert_eq!(parsed.scheme, Scheme::Http);
        assert_eq!(parsed.host, "192.168.0.196");
        assert_eq!(parsed.ipv4(), Some(Ipv4Addr::new(192, 168, 0, 196)));
        assert_eq!(parsed.explicit_port, Some(3001));
        assert_eq!(parsed.port(), 3001);
        assert_eq!(parsed.path, "/api");
    }

    #[test]
    fn scheme_case_is_ignored_and_host_case_is_kept() {
        let parsed = url("HTTPS://Example.com");
        assert_eq!(parsed.scheme, Scheme::Https);
        assert_eq!(parsed.host, "Example.com");
        assert_eq!(parsed.path, "");
        assert_eq!(parsed.explicit_port, None);
        assert_eq!(parsed.port(), 443);
    }

    #[test]
    fn http_without_a_port_uses_80() {
        let parsed = url("http://example.com/a/b");
        assert_eq!(parsed.port(), 80);
        assert_eq!(parsed.path, "/a/b");
    }

    #[test]
    fn a_bare_slash_becomes_an_empty_path() {
        let parsed = url("http://example.com/");
        assert_eq!(parsed.path, "");
    }

    #[test]
    fn only_one_trailing_slash_is_removed() {
        let parsed = url("http://example.com/api//");
        assert_eq!(parsed.path, "/api/");
    }

    #[test]
    fn port_65535_is_kept() {
        let parsed = url("http://example.com:65535");
        assert_eq!(parsed.explicit_port, Some(65535));
        assert_eq!(parsed.port(), 65535);
    }

    #[test]
    fn host_of_253_bytes_is_accepted() {
        let host = "a".repeat(253);
        let text = format!("http://{host}");
        let parsed = url(&text);
        assert_eq!(parsed.host, host);
    }

    #[test]
    fn rejected_urls() {
        let host_254 = format!("http://{}", "a".repeat(254));
        let rejected = [
            "",
            "example.com",
            "ftp://x",
            "http://",
            "http://example.com:0",
            "http://example.com:65536",
            "http://example.com:abc",
            "http://example.com/path?x",
            "http://example.com/path#x",
            "http://user@example.com",
            "http://example.com/a@b",
            "http://example.com/a b",
            "1.2.3",
            "http://1.2.3",
            "http://256.1.1.1",
            "http://192.168.0",
            "http://1.2.3.4.5",
            host_254.as_str(),
            "http://ex_ample.com",
            "http://example.com/\n",
            "http://example.com/a\"b",
            "http://example.com/a\\b",
        ];
        for text in rejected {
            assert!(parse(text).is_none(), "accepted {text}");
        }
    }
}
