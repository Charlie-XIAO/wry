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
#[cfg(not(target_os = "linux"))]
use wry::{
  dpi::{LogicalPosition, LogicalSize},
  Rect,
};
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
  webviews: Option<[WebView; 4]>,
  _proxy: EventLoopProxy<UserEvent>,
}

impl App {
  fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
    Self {
      window: None,
      webviews: None,
      _proxy: proxy,
    }
  }
}

#[cfg(not(target_os = "linux"))]
fn make_bounds(x: u32, y: u32, w: u32, h: u32) -> Rect {
  Rect {
    position: LogicalPosition::new(x, y).into(),
    size: LogicalSize::new(w, h).into(),
  }
}

impl ApplicationHandler<UserEvent> for App {
  fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
    #[cfg(not(target_os = "linux"))]
    {
      let attributes = winit::window::Window::default_attributes()
        .with_title("Multiwebview")
        .with_inner_size(winit::dpi::LogicalSize::new(800u32, 600u32));
      let window = _event_loop.create_window(attributes).unwrap();

      let scale = window.scale_factor();
      let size = window.inner_size().to_logical::<u32>(scale);
      let (w, h) = (size.width, size.height);

      let webviews = [
        WebViewBuilder::new()
          .with_bounds(make_bounds(0, 0, w / 2, h / 2))
          .with_url("https://tauri.app")
          .build(&window)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(w / 2, 0, w / 2, h / 2))
          .with_url("https://github.com/tauri-apps/wry")
          .build(&window)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(0, h / 2, w / 2, h / 2))
          .with_url("https://crates.io/crates/wry")
          .build(&window)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(w / 2, h / 2, w / 2, h / 2))
          .with_url("https://docs.rs/wry")
          .build(&window)
          .unwrap(),
      ];

      self.window = Some(window);
      self.webviews = Some(webviews);
    }

    #[cfg(target_os = "linux")]
    {
      let window = gtk::Window::builder()
        .title("Multiwebview")
        .default_width(800)
        .default_height(600)
        .build();

      let grid = gtk::Grid::new();
      grid.set_hexpand(true);
      grid.set_vexpand(true);
      window.set_child(Some(&grid));

      let cell0 = gtk::Box::new(gtk::Orientation::Vertical, 0);
      let cell1 = gtk::Box::new(gtk::Orientation::Vertical, 0);
      let cell2 = gtk::Box::new(gtk::Orientation::Vertical, 0);
      let cell3 = gtk::Box::new(gtk::Orientation::Vertical, 0);

      grid.attach(&cell0, 0, 0, 1, 1);
      grid.attach(&cell1, 1, 0, 1, 1);
      grid.attach(&cell2, 0, 1, 1, 1);
      grid.attach(&cell3, 1, 1, 1, 1);

      let webviews = [
        WebViewBuilder::new()
          .with_url("https://tauri.app")
          .build_gtk(&cell0)
          .unwrap(),
        WebViewBuilder::new()
          .with_url("https://github.com/tauri-apps/wry")
          .build_gtk(&cell1)
          .unwrap(),
        WebViewBuilder::new()
          .with_url("https://crates.io/crates/wry")
          .build_gtk(&cell2)
          .unwrap(),
        WebViewBuilder::new()
          .with_url("https://docs.rs/wry")
          .build_gtk(&cell3)
          .unwrap(),
      ];

      {
        let proxy = self._proxy.clone();
        window.connect_close_request(move |_| {
          let _ = proxy.send_event(UserEvent::GtkClosed);
          glib::Propagation::Proceed
        });
      }

      window.present();

      self.window = Some(window);
      self.webviews = Some(webviews);
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
      #[cfg(not(target_os = "linux"))]
      WindowEvent::Resized(size) => {
        if let (Some(window), Some(webviews)) = (&self.window, &self.webviews) {
          let size = size.to_logical::<u32>(window.scale_factor());
          webviews[0]
            .set_bounds(make_bounds(0, 0, size.width / 2, size.height / 2))
            .unwrap();
          webviews[1]
            .set_bounds(make_bounds(
              size.width / 2,
              0,
              size.width / 2,
              size.height / 2,
            ))
            .unwrap();
          webviews[2]
            .set_bounds(make_bounds(
              0,
              size.height / 2,
              size.width / 2,
              size.height / 2,
            ))
            .unwrap();
          webviews[3]
            .set_bounds(make_bounds(
              size.width / 2,
              size.height / 2,
              size.width / 2,
              size.height / 2,
            ))
            .unwrap();
        }
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
