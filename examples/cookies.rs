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
    let builder = WebViewBuilder::new().with_url("https://www.httpbin.org/cookies/set?foo=bar");

    #[cfg(not(target_os = "linux"))]
    let webview = {
      let attributes = winit::window::Window::default_attributes()
        .with_title("Cookies")
        .with_inner_size(winit::dpi::LogicalSize::new(800., 600.));
      let window = _event_loop.create_window(attributes).unwrap();
      let webview = builder.build(&window).unwrap();

      self.window = Some(window);
      webview
    };

    #[cfg(target_os = "linux")]
    let webview = {
      let window = gtk::Window::builder()
        .title("Cookies")
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
      webview
    };

    webview
      .set_cookie(
        cookie::Cookie::build(("foo1", "bar1"))
          .domain("www.httpbin.org")
          .path("/")
          .secure(true)
          .http_only(true)
          .max_age(cookie::time::Duration::seconds(10))
          .inner(),
      )
      .unwrap();

    let cookie_deleted = cookie::Cookie::build(("will_be_deleted", "will_be_deleted"));
    webview.set_cookie(cookie_deleted.inner()).unwrap();

    println!("Setting Cookies:");
    for cookie in webview.cookies().unwrap() {
      println!("\t{cookie}");
    }

    println!("After Deleting:");
    webview.delete_cookie(cookie_deleted.inner()).unwrap();
    for cookie in webview.cookies().unwrap() {
      println!("\t{cookie}");
    }

    self.webview = Some(webview);
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
