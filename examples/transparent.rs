// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(target_os = "linux")]
use gtk::{glib, prelude::*};
use winit::{
  application::ApplicationHandler,
  event::WindowEvent,
  event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
  window::WindowId,
};
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;
use wry::{WebView, WebViewBuilder};

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
    let builder = WebViewBuilder::new().with_transparent(true).with_html(
      r#"<html>
        <body style="background-color: transparent;"></body>
        <script>
          window.onload = () => {
            document.body.innerText = `Hello, ${navigator.userAgent}`;
            document.body.style.color = "red";
          };
        </script>
      </html>"#,
    );

    #[cfg(not(target_os = "linux"))]
    {
      #[allow(unused_mut)]
      let mut attributes = winit::window::Window::default_attributes()
        .with_title("Transparent")
        .with_transparent(true);

      let window = _event_loop.create_window(attributes).unwrap();

      #[cfg(target_os = "windows")]
      {
        use std::num::NonZeroU32;

        let context = softbuffer::Context::new(&window).unwrap();
        let mut surface = softbuffer::Surface::new(&context, &window).unwrap();
        let size = window.inner_size();
        if let (Some(width), Some(height)) =
          (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
          surface.resize(width, height).unwrap();
          let mut buffer = surface.buffer_mut().unwrap();
          buffer.fill(0x00000000);
          buffer.present().unwrap();
          println!("Filled with transparent color");
        }
      }

      let webview = builder.build(&window).unwrap();

      self.window = Some(window);
      self.webview = Some(webview);
    }

    #[cfg(target_os = "linux")]
    {
      let window = gtk::Window::builder()
        .title("Transparent")
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

fn main() {
  #[cfg(target_os = "linux")]
  gtk::init().unwrap();

  let event_loop = EventLoop::with_user_event().build().unwrap();
  let proxy = event_loop.create_proxy();
  let mut app = App::new(proxy);
  event_loop.run_app(&mut app).unwrap();
}
