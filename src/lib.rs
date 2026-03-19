#![forbid(unsafe_code)]
#![deny(
    missing_copy_implementations,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    missing_docs,
    nonstandard_style,
    unused_qualifications
)]

/*!
Async wrapper for
[`cloudflare/lol-html`](https://github.com/cloudflare/lol-html).

Since `lol-html` 2.x, [`lol_html::HtmlRewriter`] supports [`Send`]
via the [`lol_html::send`] types. This crate wraps it in an
[`AsyncRead`] implementation, feeding input from an inner async reader
and producing rewritten HTML output.

```
# use async_global_executor as your_async_executor;
# use futures_lite::{io::Cursor, AsyncReadExt};
use lol_async::html::{element, html_content::ContentType, send::Settings};

# your_async_executor::block_on(async {
let mut reader = lol_async::rewrite(
    Cursor::new(r#"<html>
<head><title>hello lol</title></head>
<body><h1>hey there</h1></body>
</html>"#),
    Settings {
        element_content_handlers: vec![element!("h1", |el| {
            el.append("<span>this was inserted</span>", ContentType::Html);
            Ok(())
        })],
        ..Settings::new_send()
    }
);

let mut buf = String::new();
reader.read_to_string(&mut buf).await?;

assert_eq!(buf, r#"<html>
<head><title>hello lol</title></head>
<body><h1>hey there<span>this was inserted</span></h1></body>
</html>"#);
# Result::<_, Box<dyn std::error::Error>>::Ok(()) }).unwrap();
```
*/

use futures_lite::{ready, AsyncRead};
use lol_html::{send::HtmlRewriter, OutputSink};
use pin_project_lite::pin_project;
use std::{
    collections::VecDeque,
    fmt::{self, Debug},
    io::{self, Read},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

pub use lol_html as html;
pub use lol_html::send::Settings;

#[derive(Clone)]
struct OutputBuffer(Arc<Mutex<VecDeque<u8>>>);

impl OutputSink for OutputBuffer {
    fn handle_chunk(&mut self, chunk: &[u8]) {
        self.0.lock().unwrap().extend(chunk);
    }
}

pin_project! {
    /// An [`AsyncRead`] adapter that streams rewritten HTML.
    ///
    /// Created by [`rewrite`]. Reads from the inner source, feeds data
    /// through the [`lol_html::HtmlRewriter`], and yields rewritten output.
    pub struct Rewriter<'h, R> {
        #[pin] source: R,
        rewriter: Option<HtmlRewriter<'h, OutputBuffer>>,
        output: OutputBuffer,
        read_buf: Vec<u8>,
    }
}

impl<R> Debug for Rewriter<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rewriter")
            .field("output_len", &self.output.0.lock().unwrap().len())
            .field("done", &self.rewriter.is_none())
            .finish()
    }
}

/// Creates an [`AsyncRead`] that streams HTML rewritten according to the
/// provided [`Settings`](lol_html::send::Settings).
///
/// See the [crate-level documentation](crate) for an example.
pub fn rewrite<'h, R: AsyncRead>(source: R, settings: Settings<'h, '_>) -> Rewriter<'h, R> {
    let output = OutputBuffer(Arc::new(Mutex::new(VecDeque::new())));
    let rewriter = HtmlRewriter::new(settings, output.clone());
    Rewriter {
        source,
        rewriter: Some(rewriter),
        output,
        read_buf: vec![0; 1024],
    }
}

impl<R: AsyncRead> AsyncRead for Rewriter<'_, R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();

        loop {
            // Drain any buffered output first
            {
                let mut output = this.output.0.lock().unwrap();
                if !output.is_empty() {
                    let n = Read::read(&mut *output, buf)?;
                    return Poll::Ready(Ok(n));
                }
            }

            // If the rewriter is done and no output remains, signal EOF
            let rewriter = match this.rewriter.as_mut() {
                Some(r) => r,
                None => return Poll::Ready(Ok(0)),
            };

            // Read from the source and feed through the rewriter
            match ready!(this.source.as_mut().poll_read(cx, this.read_buf)) {
                Ok(0) => {
                    this.rewriter
                        .take()
                        .unwrap()
                        .end()
                        .map_err(io::Error::other)?;
                    // Loop back to drain any final output
                }

                Ok(n) => {
                    rewriter
                        .write(&this.read_buf[..n])
                        .map_err(io::Error::other)?;
                    // Loop back to drain output
                }

                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }
}
