use futures_lite::{AsyncRead, AsyncReadExt, io::Cursor};
use lol_async::{
    html::{element, html_content::ContentType, send::Settings},
    rewrite,
};
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

// -- helpers ------------------------------------------------------------------

async fn read_with_buffers_of_size<R>(reader: &mut R, size: usize) -> Result<String>
where
    R: AsyncRead + Unpin,
{
    let mut return_buffer = vec![];
    loop {
        let mut buf = vec![0; size];
        match reader.read(&mut buf).await? {
            0 => break Ok(String::from_utf8_lossy(&return_buffer).into()),
            bytes_read => return_buffer.extend_from_slice(&buf[..bytes_read]),
        }
    }
}

fn passthrough_settings() -> Settings<'static, 'static> {
    Settings::new_send()
}

fn simple_append_settings() -> Settings<'static, 'static> {
    Settings {
        element_content_handlers: vec![element!("h1", |el| {
            el.append("<span>inserted</span>", ContentType::Html);
            Ok(())
        })],
        ..Settings::new_send()
    }
}

/// An AsyncRead that yields `data` in fixed-size chunks, simulating a slow or
/// fragmented source (e.g. a network stream).
struct ChunkedReader {
    data: Vec<u8>,
    pos: usize,
    chunk_size: usize,
}

impl ChunkedReader {
    fn new(data: impl Into<Vec<u8>>, chunk_size: usize) -> Self {
        Self {
            data: data.into(),
            pos: 0,
            chunk_size,
        }
    }
}

impl AsyncRead for ChunkedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let remaining = &self.data[self.pos..];
        if remaining.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = remaining.len().min(self.chunk_size).min(buf.len());
        buf[..n].copy_from_slice(&remaining[..n]);
        self.pos += n;
        Poll::Ready(Ok(n))
    }
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    async_global_executor::block_on(f)
}

// -- tests --------------------------------------------------------------------

#[test]
fn empty_input() {
    run(async {
        let mut r = rewrite(Cursor::new(b""), passthrough_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "");
    });
}

#[test]
fn passthrough_no_handlers() {
    run(async {
        let html = "<html><body><p>hello</p></body></html>";
        let mut r = rewrite(Cursor::new(html), passthrough_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, html);
    });
}

#[test]
fn simple_element_append() {
    run(async {
        let mut r = rewrite(Cursor::new("<h1>hi</h1>"), simple_append_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<h1>hi<span>inserted</span></h1>");
    });
}

#[test]
fn element_removal() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<div><span class=\"remove\">gone</span><p>kept</p></div>"),
            Settings {
                element_content_handlers: vec![element!("span.remove", |el| {
                    el.remove();
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<div><p>kept</p></div>");
    });
}

#[test]
fn element_replacement() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<div><old>content</old></div>"),
            Settings {
                element_content_handlers: vec![element!("old", |el| {
                    el.replace("<new>replaced</new>", ContentType::Html);
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<div><new>replaced</new></div>");
    });
}

#[test]
fn set_inner_content() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<p>old</p>"),
            Settings {
                element_content_handlers: vec![element!("p", |el| {
                    el.set_inner_content("new", ContentType::Text);
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<p>new</p>");
    });
}

#[test]
fn attribute_rewriting() {
    run(async {
        let mut r = rewrite(
            Cursor::new(r#"<a href="http://example.com">link</a>"#),
            Settings {
                element_content_handlers: vec![element!("a[href]", |el| {
                    let href = el.get_attribute("href").unwrap().replace("http:", "https:");
                    el.set_attribute("href", &href).unwrap();
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, r#"<a href="https://example.com">link</a>"#);
    });
}

#[test]
fn multiple_handlers() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<h1>title</h1><p>body</p>"),
            Settings {
                element_content_handlers: vec![
                    element!("h1", |el| {
                        el.set_inner_content("NEW TITLE", ContentType::Text);
                        Ok(())
                    }),
                    element!("p", |el| {
                        el.before("<hr>", ContentType::Html);
                        Ok(())
                    }),
                ],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<h1>NEW TITLE</h1><hr><p>body</p>");
    });
}

#[test]
fn before_and_after_insertion() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<p>middle</p>"),
            Settings {
                element_content_handlers: vec![element!("p", |el| {
                    el.before("<!--before-->", ContentType::Html);
                    el.after("<!--after-->", ContentType::Html);
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, "<!--before--><p>middle</p><!--after-->");
    });
}

// -- buffer size sweep tests --------------------------------------------------

#[test]
fn small_buffer_sweep_passthrough() {
    let html = "<html><head><title>hi</title></head><body><p>hello world</p></body></html>";
    for size in 1..=64 {
        run(async {
            let mut r = rewrite(Cursor::new(html), passthrough_settings());
            let result = read_with_buffers_of_size(&mut r, size).await.unwrap();
            assert_eq!(result, html, "failed with read buffer size {size}");
        });
    }
}

#[test]
fn small_buffer_sweep_with_rewriting() {
    let input = "<div><h1>hello</h1></div>";
    let expected = "<div><h1>hello<span>inserted</span></h1></div>";
    for size in 1..=64 {
        run(async {
            let mut r = rewrite(Cursor::new(input), simple_append_settings());
            let result = read_with_buffers_of_size(&mut r, size).await.unwrap();
            assert_eq!(result, expected, "failed with read buffer size {size}");
        });
    }
}

// -- large input (exceeds internal 1024-byte read buffer) ---------------------

#[test]
fn large_input_passthrough() {
    run(async {
        let body: String = (0..200).map(|i| format!("<p>paragraph {i}</p>")).collect();
        let html = format!("<html><body>{body}</body></html>");
        assert!(
            html.len() > 2048,
            "test input should exceed internal buffer"
        );

        let mut r = rewrite(Cursor::new(html.as_str()), passthrough_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, html);
    });
}

#[test]
fn large_input_with_rewriting() {
    run(async {
        let body: String = (0..200).map(|i| format!("<p>paragraph {i}</p>")).collect();
        let html = format!("<html><body>{body}</body></html>");
        assert!(html.len() > 2048);

        let expected = html.replace("<p>", "<p class=\"styled\">");

        let mut r = rewrite(
            Cursor::new(html.as_str()),
            Settings {
                element_content_handlers: vec![element!("p", |el| {
                    el.set_attribute("class", "styled").unwrap();
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, expected);
    });
}

#[test]
fn large_input_small_buffer_sweep() {
    let body: String = (0..100).map(|i| format!("<li>{i}</li>")).collect();
    let html = format!("<ul>{body}</ul>");
    assert!(html.len() > 1024);

    for size in [1, 2, 3, 7, 13, 64, 128, 512] {
        run(async {
            let mut r = rewrite(Cursor::new(html.as_str()), passthrough_settings());
            let result = read_with_buffers_of_size(&mut r, size).await.unwrap();
            assert_eq!(result, html, "failed with read buffer size {size}");
        });
    }
}

// -- chunked source (small input chunks) --------------------------------------

#[test]
fn chunked_source_one_byte_at_a_time() {
    run(async {
        let html = "<div><h1>hello</h1><p>world</p></div>";
        let mut r = rewrite(ChunkedReader::new(html, 1), passthrough_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, html);
    });
}

#[test]
fn chunked_source_with_rewriting() {
    run(async {
        let html = "<div><h1>hello</h1></div>";
        let expected = "<div><h1>hello<span>inserted</span></h1></div>";
        let mut r = rewrite(ChunkedReader::new(html, 3), simple_append_settings());
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, expected);
    });
}

#[test]
fn chunked_source_and_small_read_buffer() {
    let html = "<ul><li>a</li><li>b</li><li>c</li></ul>";
    for source_chunk in [1, 2, 5, 10] {
        for read_buf in [1, 2, 5, 10] {
            run(async {
                let mut r = rewrite(
                    ChunkedReader::new(html, source_chunk),
                    passthrough_settings(),
                );
                let result = read_with_buffers_of_size(&mut r, read_buf).await.unwrap();
                assert_eq!(
                    result, html,
                    "failed with source_chunk={source_chunk}, read_buf={read_buf}"
                );
            });
        }
    }
}

// -- output expansion (rewriting produces much more output than input) --------

#[test]
fn output_much_larger_than_input() {
    run(async {
        let big_insert = "x".repeat(4096);
        let big_insert_clone = big_insert.clone();
        let mut r = rewrite(
            Cursor::new("<p></p>"),
            Settings {
                element_content_handlers: vec![element!("p", move |el| {
                    el.set_inner_content(&big_insert_clone, ContentType::Text);
                    Ok(())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        r.read_to_string(&mut buf).await.unwrap();
        assert_eq!(buf, format!("<p>{big_insert}</p>"));
    });
}

#[test]
fn output_expansion_small_read_buffer() {
    let big_insert = "y".repeat(2048);
    for size in [1, 7, 64, 256] {
        let big_insert_clone = big_insert.clone();
        run(async {
            let insert = big_insert_clone.clone();
            let mut r = rewrite(
                Cursor::new("<span></span>"),
                Settings {
                    element_content_handlers: vec![element!("span", move |el| {
                        el.set_inner_content(&insert, ContentType::Text);
                        Ok(())
                    })],
                    ..Settings::new_send()
                },
            );
            let result = read_with_buffers_of_size(&mut r, size).await.unwrap();
            assert_eq!(
                result,
                format!("<span>{big_insert_clone}</span>"),
                "failed with read buffer size {size}"
            );
        });
    }
}

// -- error propagation --------------------------------------------------------

#[test]
fn handler_error_propagates() {
    run(async {
        let mut r = rewrite(
            Cursor::new("<p>hi</p>"),
            Settings {
                element_content_handlers: vec![element!("p", |_el| {
                    Err("handler error".into())
                })],
                ..Settings::new_send()
            },
        );
        let mut buf = String::new();
        let err = r.read_to_string(&mut buf).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(
            err.to_string().contains("handler error"),
            "unexpected error message: {}",
            err
        );
    });
}

struct ErrorReader;

impl AsyncRead for ErrorReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "source error",
        )))
    }
}

#[test]
fn source_read_error_propagates() {
    run(async {
        let mut r = rewrite(ErrorReader, passthrough_settings());
        let mut buf = String::new();
        let err = r.read_to_string(&mut buf).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    });
}

// -- misc ---------------------------------------------------------------------

#[test]
fn debug_impl() {
    let r = rewrite(Cursor::new("<p>hi</p>"), passthrough_settings());
    let debug = format!("{r:?}");
    assert!(debug.contains("Rewriter"));
    assert!(debug.contains("done: false"));
}

#[test]
fn rewriter_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<lol_async::Rewriter<'_, Cursor<&[u8]>>>();
}
