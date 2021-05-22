#![forbid(unsafe_code)]
#![deny(
    missing_copy_implementations,
    missing_crate_level_docs,
    missing_debug_implementations,
    missing_docs,
    nonstandard_style,
    unused_qualifications
)]

/*!
This crate is an effort to adapt
[`cloudflare/lol-html`](https://github.com/cloudflare/lol-html) for an
async context. Unfortunately, due to lol-html's design, the wrapped
api is not ideal. In particular, the [`lol_html::HtmlRewriter`] is
`!Send`, which means that we have some contortions in order to use
this library in an async context that expects most futures to be
`Send`. This crate addresses this by returning two types: A
[`LolFuture`] that must be polled on whatever thread calls [`rewrite`]
and a [`LolReader`] that is [`AsyncRead`] and can be moved around and
sent between threads as needed.

## ❗ Due to this design, it is necessary to poll the [`LolFuture`] in
addition to reading from the [`LolReader`] ❗

## Improvements

Improvements to the design of this crate are very welcome. I don't
have a lot of experience working around `!Send` types in an async
context, and although this crate achieved the result I needed, I hate
it. Please open a PR or write a better crate! Alternatively, if you
know someone at cloudflare, maybe arms can be twisted to adapt
`lol-html` for async rust.

```
# use async_global_executor::{block_on, spawn_local};
# use futures_lite::{io::Cursor, AsyncReadExt};

use lol_async::html::{element, html_content::ContentType, Settings};
# block_on(async {
let (fut, mut reader) = lol_async::rewrite(
    Cursor::new("<html><head><title>hello lol</title></head><body><h1>hey there</h1></body></html>"),
    Settings {
        element_content_handlers: vec![element!("h1", |el| {
            el.append("<span>this was inserted</span>", ContentType::Html);
            Ok(())
        })],
        ..Settings::default()
    }
);

let handle = spawn_local(fut);
let mut buf = String::new();
reader.read_to_string(&mut buf).await.unwrap();
handle.await.unwrap();
assert_eq!(buf, "<html><head><title>hello lol</title></head><body><h1>hey there<span>this was inserted</span></h1></body></html>");
# });
```


*/
use atomic_waker::AtomicWaker;
use futures_lite::{ready, AsyncRead};
use lol_html::{HtmlRewriter, OutputSink, Settings};
use pin_project_lite::pin_project;
use ringbuf::{Consumer, Producer, RingBuffer};
use std::{
    fmt::{self, Debug},
    future::Future,
    io::Result,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

pub use lol_html as html;

pin_project! {
    /**
    `await` this [`Future`] to drive the html rewriting process. The
    LolFuture contains the [`HtmlRewriter`] and as a result is
    `!Send`, so it must be spawned locally.
    */
    pub struct LolFuture<'h, R> {
        #[pin] source: R,
        rewriter: Option<HtmlRewriter<'h, LolOutputter>>,
        buffer: Vec<u8>
    }
}

impl<R> Debug for LolFuture<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LolFuture")
            .field("buffer", &self.buffer)
            .finish()
    }
}

/**
An [`AsyncRead`] type that will yield the rewritten html. LolReader is
Send, allowing it to be used on a different thread from the paired
[`LolFuture`]. Please note that reading from LolReader does not drive the
LolHtml rewriter. Awaiting the [`LolFuture`] is also necessary.
*/

pub struct LolReader {
    waker: Arc<AtomicWaker>,
    done: Arc<AtomicBool>,
    receiver: Consumer<u8>,
}

impl Debug for LolReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LolReader")
            .field("waker", &self.waker)
            .field("done", &self.done)
            .finish()
    }
}

struct LolOutputter {
    done: Arc<AtomicBool>,
    waker: Arc<AtomicWaker>,
    sender: Producer<u8>,
}

impl OutputSink for LolOutputter {
    fn handle_chunk(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            self.done.store(true, Ordering::SeqCst);
        } else {
            self.sender.push_slice(chunk);
        }

        self.waker.wake();
    }
}

/**
This function is the primary entrypoint for `lol-async`. It takes a
data `Source` that is [`AsyncRead`] and a [`Settings`] that describes
the desired rewriting logic. It returns a !Send [`LolFuture`] future
that drives the rewriter on the current thread and a [`LolReader`]
that is Send and can be used anywhere an [`AsyncRead`] would be
used. The html content yielded by the [`LolReader`] will be rewritten
according to the rules specified in the Settings.
*/
pub fn rewrite<'h, 's, Source>(
    source: Source,
    settings: Settings<'h, 's>,
) -> (LolFuture<'h, Source>, LolReader)
where
    Source: AsyncRead,
{
    let (sender, receiver) = RingBuffer::new(1024 * 8).split();
    let waker = Arc::new(AtomicWaker::new());
    let done = Arc::new(AtomicBool::new(false));

    let output_sink = LolOutputter {
        waker: waker.clone(),
        done: done.clone(),
        sender,
    };

    let rewriter = HtmlRewriter::new(settings, output_sink);

    let future = LolFuture {
        source,
        rewriter: Some(rewriter),
        buffer: vec![0; 1024],
    };

    let reader = LolReader {
        waker,
        done,
        receiver,
    };

    (future, reader)
}

impl AsyncRead for LolReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        if self.receiver.is_empty() && self.done.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(0));
        }

        match self.receiver.pop_slice(buf) {
            0 => {
                self.waker.register(cx.waker());
                Poll::Pending
            }

            ready_bytes => Poll::Ready(Ok(ready_bytes)),
        }
    }
}

impl<Source: AsyncRead> Future for LolFuture<'_, Source> {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.rewriter {
                Some(rewriter) => match ready!(this.source.as_mut().poll_read(cx, this.buffer))? {
                    0 => {
                        this.rewriter.take().unwrap().end().unwrap();
                    }

                    new_bytes => {
                        rewriter.write(&this.buffer[..new_bytes]).unwrap();
                    }
                },

                None => return Poll::Ready(Ok(())),
            }
        }
    }
}
