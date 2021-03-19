use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use futures_lite::{ready, AsyncRead};
pub use lol_html as html;
use lol_html::{HtmlRewriter, OutputSink, Settings};

use pin_project_lite::pin_project;

use ringbuf::{Consumer, Producer, RingBuffer};

pin_project! {
    pub struct Lol<'h, R> {
        #[pin] inner: R,
        rewriter: Option<HtmlRewriter<'h, LolOutputter>>,
        buffer: Vec<u8>
    }
}

pub struct LolReader {
    waker: Arc<AtomicWaker>,
    done: Arc<AtomicBool>,
    receiver: Consumer<u8>,
}

struct LolOutputter {
    done: Arc<AtomicBool>,
    waker: Arc<AtomicWaker>,
    sender: Producer<u8>,
}

impl OutputSink for LolOutputter {
    fn handle_chunk(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            self.done.store(true, std::sync::atomic::Ordering::SeqCst);
        } else {
            self.sender.push_slice(chunk);
        }
        self.waker.wake();
    }
}

pub fn lol<'h, 's, R>(inner: R, settings: Settings<'h, 's>) -> (Lol<'h, R>, LolReader) {
    let (sender, receiver) = RingBuffer::new(1024 * 8).split();
    let waker = Arc::new(AtomicWaker::new());
    let done = Arc::new(AtomicBool::new(false));

    let outputter = LolOutputter {
        waker: waker.clone(),
        done: done.clone(),
        sender,
    };

    let rewriter = HtmlRewriter::new(settings, outputter);

    let fut = Lol {
        inner,
        rewriter: Some(rewriter),
        buffer: vec![0; 1024],
    };

    let reader = LolReader {
        waker,
        receiver,
        done,
    };

    (fut, reader)
}

impl AsyncRead for LolReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        if self.receiver.is_empty() && self.done.load(std::sync::atomic::Ordering::SeqCst) {
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

impl<R: AsyncRead> Future for Lol<'_, R> {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.rewriter {
                Some(rewriter) => match ready!(this.inner.as_mut().poll_read(cx, this.buffer))? {
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

#[cfg(test)]
mod tests {
    use futures_lite::{io::Cursor, AsyncReadExt};
    use lol_html::element;
    use lol_html::html_content::ContentType;

    use super::*;

    #[test]
    fn it_works() {
        async_global_executor::block_on(async {
            let (fut, mut reader) = lol(
                Cursor::new(
                    "<html><head><title>hello lol</title></head><body><h1>hey there</h1></body></html>",
                ),
                Settings {
                    element_content_handlers: vec![element!("h1", |el| {
                        el.append("<span>only one</span>", ContentType::Html);
                        Ok(())
                    })],
                    ..Settings::default()
                }
            );

            let handle = async_global_executor::spawn_local(fut);
            let mut buf = String::new();
            reader.read_to_string(&mut buf).await.unwrap();
            handle.await.unwrap();
            println!("{}", buf);
        });
    }
}
