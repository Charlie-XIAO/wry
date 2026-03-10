// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(target_os = "linux")]
use gtk::{glib, prelude::*};
use std::collections::HashMap;
use winit::{
  application::ApplicationHandler,
  event::WindowEvent,
  event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
  window::WindowId,
};
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;
use wry::{WebView, WebViewBuilder};

enum UserEvent {
  NewWindow,
  CloseWindow(u64),
  NewTitle(u64, String),
}

struct App {
  #[cfg(not(target_os = "linux"))]
  windows: HashMap<u64, (winit::window::Window, WebView)>,
  #[cfg(not(target_os = "linux"))]
  window_id_map: HashMap<WindowId, u64>,
  #[cfg(target_os = "linux")]
  windows: HashMap<u64, (gtk::Window, WebView)>,
  next_id: u64,
  proxy: EventLoopProxy<UserEvent>,
}

impl App {
  fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
    Self {
      windows: HashMap::new(),
      #[cfg(not(target_os = "linux"))]
      window_id_map: HashMap::new(),
      next_id: 0,
      proxy,
    }
  }

  fn new_window(&mut self, _event_loop: &ActiveEventLoop) {
    let id = self.next_id;
    self.next_id += 1;

    let title = format!("Window {}", self.windows.len() + 1);
    let proxy = self.proxy.clone();

    let builder = WebViewBuilder::new()
      .with_html(
        r#"
        <button onclick="window.ipc.postMessage('new-window')">Open a new window</button>
        <button onclick="window.ipc.postMessage('close')">Close current window</button>
        <input oninput="window.ipc.postMessage(`change-title:${this.value}`)" />
        "#,
      )
      .with_ipc_handler(move |req| {
        let body = req.body();
        match body.as_str() {
          "new-window" => {
            let _ = proxy.send_event(UserEvent::NewWindow);
          }
          "close" => {
            let _ = proxy.send_event(UserEvent::CloseWindow(id));
          }
          other => {
            if let Some(title) = other.strip_prefix("change-title:") {
              let _ = proxy.send_event(UserEvent::NewTitle(id, title.to_string()));
            }
          }
        }
      });

    #[cfg(not(target_os = "linux"))]
    {
      let attributes = winit::window::Window::default_attributes()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(800., 600.));
      let window = _event_loop.create_window(attributes).unwrap();
      let webview = builder.build(&window).unwrap();

      let window_id = window.id();
      self.windows.insert(window_id, (window, webview));
      self.window_id_map.insert(window_id, id);
    }

    #[cfg(target_os = "linux")]
    {
      let window = gtk::Window::builder()
        .title(title.as_str())
        .default_width(800)
        .default_height(600)
        .build();

      {
        let proxy = self.proxy.clone();
        window.connect_close_request(move |_| {
          let _ = proxy.send_event(UserEvent::CloseWindow(id));
          glib::Propagation::Proceed
        });
      }

      let webview = builder.build_gtk(&window).unwrap();
      window.present();

      self.windows.insert(id, (window, webview));
    }
  }
}

impl ApplicationHandler<UserEvent> for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    self.new_window(event_loop);
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
        if let Some(&id) = self.window_id_map.get(&_window_id) {
          self.windows.remove(&id);
          self.window_id_map.remove(&_window_id);
          if self.windows.is_empty() {
            _event_loop.exit();
          }
        }
      }
      _ => {}
    }
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
      UserEvent::NewWindow => {
        self.new_window(event_loop);
      }
      UserEvent::CloseWindow(id) => {
        if let Some((_window, _)) = self.windows.remove(&id) {
          #[cfg(target_os = "linux")]
          _window.close();
        }

        #[cfg(not(target_os = "linux"))]
        self.window_id_map.retain(|_, v| *v != id);

        if self.windows.is_empty() {
          event_loop.exit();
        }
      }
      UserEvent::NewTitle(id, title) => {
        if let Some(entry) = self.windows.get(&id) {
          #[cfg(not(target_os = "linux"))]
          entry.0.set_title(&title);
          #[cfg(target_os = "linux")]
          entry.0.set_title(Some(&title));
        }
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
