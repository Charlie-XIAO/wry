// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(feature = "protocol")]
fn main() {
  use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
  };

  #[cfg(target_os = "linux")]
  use gtk::{glib, prelude::*};
  use http::{header, StatusCode};
  use http_range::HttpRange;
  use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::WindowId,
  };
  #[cfg(target_os = "linux")]
  use wry::WebViewBuilderExtUnix;
  use wry::{
    http::{header::*, Request, Response},
    WebView, WebViewBuilder,
  };

  #[derive(Debug)]
  enum UserEvent {
    #[cfg(target_os = "linux")]
    GtkClosed,
  }

  struct App {
    #[cfg(not(target_os = "linux"))]
    window: Option<winit::window::Window>,
    #[cfg(target_os = "linux")]
    window: Option<gtk::Window>,
    webview: Option<WebView>,
    _proxy: EventLoopProxy<UserEvent>,
  }

  impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
      Self {
        window: None,
        webview: None,
        _proxy: proxy,
      }
    }
  }

  impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
      let builder = WebViewBuilder::new()
        .with_custom_protocol(
          "wry".into(),
          move |_, request| match wry_protocol(request) {
            Ok(r) => r.map(Into::into),
            Err(e) => http::Response::builder()
              .header(CONTENT_TYPE, "text/plain")
              .status(500)
              .body(e.to_string().as_bytes().to_vec())
              .unwrap()
              .map(Into::into),
          },
        )
        .with_custom_protocol(
          "stream".into(),
          move |_webview_id, request| match stream_protocol(request) {
            Ok(r) => r.map(Into::into),
            Err(e) => http::Response::builder()
              .header(CONTENT_TYPE, "text/plain")
              .status(500)
              .body(e.to_string().as_bytes().to_vec())
              .unwrap()
              .map(Into::into),
          },
        )
        .with_url("wry://localhost");

      #[cfg(not(target_os = "linux"))]
      {
        let attributes = winit::window::Window::default_attributes()
          .with_title("Streaming")
          .with_inner_size(winit::dpi::LogicalSize::new(800., 600.));
        let window = _event_loop.create_window(attributes).unwrap();
        let webview = builder.build(&window).unwrap();

        self.window = Some(window);
        self.webview = Some(webview);
      }

      #[cfg(target_os = "linux")]
      {
        let window = gtk::Window::builder()
          .title("Streaming")
          .default_width(800)
          .default_height(600)
          .build();

        {
          let proxy = self._proxy.clone();
          window.connect_close_request(move |_| {
            let _ = proxy.send_event(UserEvent::GtkClosed);
            glib::Propagation::Proceed
          });
        }

        let webview = builder.build_gtk(&window).unwrap();
        window.present();

        self.window = Some(window);
        self.webview = Some(webview);
      }
    }

    fn window_event(
      &mut self,
      _event_loop: &ActiveEventLoop,
      _window_id: WindowId,
      event: WindowEvent,
    ) {
      match event {
        #[cfg(not(target_os = "linux"))]
        WindowEvent::CloseRequested => {
          _event_loop.exit();
        }
        _ => {}
      }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
      match event {
        #[cfg(target_os = "linux")]
        UserEvent::GtkClosed => {
          _event_loop.exit();
        }
      }
    }
  }

  fn wry_protocol(
    request: Request<Vec<u8>>,
  ) -> Result<http::Response<Vec<u8>>, Box<dyn std::error::Error>> {
    let path = request.uri().path();
    // Read the file content from file path
    let root = PathBuf::from("examples/streaming");
    let path = if path == "/" {
      "index.html"
    } else {
      &path[1..] // removing leading slash
    };
    let content = std::fs::read(std::fs::canonicalize(root.join(path))?)?;

    let mimetype = if path.ends_with(".html") || path == "/" {
      "text/html"
    } else if path.ends_with(".js") {
      "text/javascript"
    } else {
      unimplemented!();
    };

    Response::builder()
      .header(CONTENT_TYPE, mimetype)
      .body(content)
      .map_err(Into::into)
  }

  fn stream_protocol(
    request: http::Request<Vec<u8>>,
  ) -> Result<http::Response<Vec<u8>>, Box<dyn std::error::Error>> {
    // skip leading `/`
    let path = percent_encoding::percent_decode(&request.uri().path().as_bytes()[1..])
      .decode_utf8_lossy()
      .to_string();

    let mut file = std::fs::File::open(path)?;

    // get file length
    let len = {
      let old_pos = file.stream_position()?;
      let len = file.seek(SeekFrom::End(0))?;
      file.seek(SeekFrom::Start(old_pos))?;
      len
    };

    let mut resp = Response::builder().header(CONTENT_TYPE, "video/mp4");

    // if the webview sent a range header, we need to send a 206 in return
    // Actually only macOS and Windows are supported. Linux will ALWAYS return empty headers.
    let http_response = if let Some(range_header) = request.headers().get("range") {
      let not_satisfiable = || {
        Response::builder()
          .status(StatusCode::RANGE_NOT_SATISFIABLE)
          .header(header::CONTENT_RANGE, format!("bytes */{len}"))
          .body(vec![])
      };

      // parse range header
      let ranges = if let Ok(ranges) = HttpRange::parse(range_header.to_str()?, len) {
        ranges
          .iter()
          // map the output back to spec range <start-end>, example: 0-499
          .map(|r| (r.start, r.start + r.length - 1))
          .collect::<Vec<_>>()
      } else {
        return Ok(not_satisfiable()?);
      };

      // The maximum bytes we send in one range
      const MAX_LEN: u64 = 1000 * 1024;

      if ranges.len() == 1 {
        let &(start, mut end) = ranges.first().unwrap();

        // check if a range is not satisfiable
        //
        // this should be already taken care of by HttpRange::parse
        // but checking here again for extra assurance
        if start >= len || end >= len || end < start {
          return Ok(not_satisfiable()?);
        }

        // adjust end byte for MAX_LEN
        end = start + (end - start).min(len - start).min(MAX_LEN - 1);

        // calculate number of bytes needed to be read
        let bytes_to_read = end + 1 - start;

        // allocate a buf with a suitable capacity
        let mut buf = Vec::with_capacity(bytes_to_read as usize);
        // seek the file to the starting byte
        file.seek(SeekFrom::Start(start))?;
        // read the needed bytes
        file.take(bytes_to_read).read_to_end(&mut buf)?;

        resp = resp.header(CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
        resp = resp.header(CONTENT_LENGTH, end + 1 - start);
        resp = resp.status(StatusCode::PARTIAL_CONTENT);
        resp.body(buf)
      } else {
        let mut buf = Vec::new();
        let ranges = ranges
          .iter()
          .filter_map(|&(start, mut end)| {
            // filter out unsatisfiable ranges
            //
            // this should be already taken care of by HttpRange::parse
            // but checking here again for extra assurance
            if start >= len || end >= len || end < start {
              None
            } else {
              // adjust end byte for MAX_LEN
              end = start + (end - start).min(len - start).min(MAX_LEN - 1);
              Some((start, end))
            }
          })
          .collect::<Vec<_>>();

        let boundary = random_boundary();
        let boundary_sep = format!("\r\n--{boundary}\r\n");
        let boundary_closer = format!("\r\n--{boundary}\r\n");

        resp = resp.header(
          CONTENT_TYPE,
          format!("multipart/byteranges; boundary={boundary}"),
        );

        for (end, start) in ranges {
          // a new range is being written, write the range boundary
          buf.write_all(boundary_sep.as_bytes())?;

          // write the needed headers `Content-Type` and `Content-Range`
          buf.write_all(format!("{CONTENT_TYPE}: video/mp4\r\n").as_bytes())?;
          buf.write_all(format!("{CONTENT_RANGE}: bytes {start}-{end}/{len}\r\n").as_bytes())?;

          // write the separator to indicate the start of the range body
          buf.write_all("\r\n".as_bytes())?;

          // calculate number of bytes needed to be read
          let bytes_to_read = end + 1 - start;

          let mut local_buf = vec![0_u8; bytes_to_read as usize];
          file.seek(SeekFrom::Start(start))?;
          file.read_exact(&mut local_buf)?;
          buf.extend_from_slice(&local_buf);
        }
        // all ranges have been written, write the closing boundary
        buf.write_all(boundary_closer.as_bytes())?;

        resp.body(buf)
      }
    } else {
      resp = resp.header(CONTENT_LENGTH, len);
      let mut buf = Vec::with_capacity(len as usize);
      file.read_to_end(&mut buf)?;
      resp.body(buf)
    };

    http_response.map_err(Into::into)
  }

  fn random_boundary() -> String {
    let mut x = [0_u8; 30];
    getrandom::fill(&mut x).expect("failed to get random bytes");
    (x[..])
      .iter()
      .map(|&x| format!("{x:x}"))
      .fold(String::new(), |mut a, x| {
        a.push_str(x.as_str());
        a
      })
  }

  #[cfg(target_os = "linux")]
  {
    gtk::init().unwrap();
    println!("This example might not work properly on Linux. See also:");
    println!("- WebKit bug: https://bugs.webkit.org/show_bug.cgi?id=146351");
    println!("- Tauri issue: https://github.com/tauri-apps/tauri/issues/3725");
  }

  let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
  let proxy = event_loop.create_proxy();
  let mut app = App::new(proxy);
  event_loop.run_app(&mut app).unwrap();
}

#[cfg(not(feature = "protocol"))]
fn main() {
  println!("The protocol feature is required to run this example");
}
