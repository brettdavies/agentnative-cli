"""One TRACE line per network request when ANC_WEB_TRACE is set, inert otherwise."""
import pathlib

p = pathlib.Path('src/web_audit/transport.rs')
s = p.read_text()
old = '            if !retryable {\n'
assert old in s
new = old + '''                if std::env::var_os("ANC_WEB_TRACE").is_some() {
                    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
                    let t0 = *T0.get_or_init(Instant::now);
                    let what = match &outcome {
                        Ok(r) => r.status.to_string(),
                        Err(e) => format!("err:{e:?}").chars().take(40).collect(),
                    };
                    eprintln!(
                        "TRACE\\t{:.3}\\t{:.3}\\t{}\\t{}\\t{}\\t{}",
                        started.saturating_duration_since(t0).as_secs_f64(),
                        started.elapsed().as_secs_f64(),
                        attempt,
                        request.method,
                        request.url,
                        what
                    );
                }
'''
p.write_text(s.replace(old, new, 1))
