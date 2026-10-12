//! Téléchargements (mises à jour, modèles, police) : HTTPS avec les certificats du système et le
//! proxy de l'environnement (HTTPS_PROXY), comme curl et urllib dans la version Python.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

pub fn agent(timeout: Duration) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new().timeout(timeout).user_agent("Mozilla/5.0 (Auto-montage)");
    for var in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(p) = std::env::var(var) {
            if let Ok(proxy) = ureq::Proxy::new(&p) {
                b = b.proxy(proxy);
                break;
            }
        }
    }
    b.build()
}

/// GET : (code HTTP, corps). Erreur réseau : Err(message).
pub fn get(url: &str, headers: &[(&str, &str)], timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    let mut req = agent(timeout).get(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let mut body = vec![];
            let _ = r.into_reader().take(1 << 20).read_to_end(&mut body);
            return Ok((code, body));
        }
        Err(e) => return Err(e.to_string()),
    };
    let code = resp.status();
    let mut body = vec![];
    resp.into_reader().read_to_end(&mut body).map_err(|e| e.to_string())?;
    Ok((code, body))
}

/// Télécharge dans un fichier, au fil de l'eau (écrit sous un nom provisoire puis renommé).
/// Erreur si le code HTTP n'est pas 2xx.
pub fn download(url: &str, out: &Path, timeout: Duration) -> Result<u64, String> {
    let resp = match agent(timeout).get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code} pour {url}")),
        Err(e) => return Err(e.to_string()),
    };
    let part = out.with_file_name(format!("{}.part", out.file_name().unwrap().to_string_lossy()));
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let n = std::io::copy(&mut resp.into_reader(), &mut file).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })?;
    drop(file);
    std::fs::rename(&part, out).map_err(|e| e.to_string())?;
    Ok(n)
}
