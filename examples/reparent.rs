// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#[cfg(target_os = "linux")]
use gtk::{gdk, glib, prelude::*};
use winit::{
  application::ApplicationHandler,
  event::WindowEvent,
  event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
  window::WindowId,
};
#[cfg(not(target_os = "linux"))]
use winit::{event::ElementState, keyboard::Key};
use wry::{WebView, WebViewBuilder};
#[cfg(target_os = "linux")]
use wry::{WebViewBuilderExtUnix, WebViewExtUnix};

#[cfg(target_os = "windows")]
use wry::WebViewExtWindows;
#[cfg(target_os = "macos")]
use {objc2_app_kit::NSWindow, wry::WebViewExtMacOS};

#[derive(Debug)]
enum UserEvent {
  #[cfg(target_os = "linux")]
  GtkClosed,
  #[cfg(target_os = "linux")]
  Reparent,
}

struct App {
  #[cfg(not(target_os = "linux"))]
  window_a: Option<winit::window::Window>,
  #[cfg(not(target_os = "linux"))]
  window_b: Option<winit::window::Window>,
  #[cfg(target_os = "linux")]
  window_a: Option<gtk::Window>,
  #[cfg(target_os = "linux")]
  window_b: Option<gtk::Window>,
  webview: Option<WebView>,
  on_window_b: bool,
  _proxy: EventLoopProxy<UserEvent>,
}

impl App {
  fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
    Self {
      window_a: None,
      window_b: None,
      webview: None,
      on_window_b: false,
      _proxy: proxy,
    }
  }
}

impl ApplicationHandler<UserEvent> for App {
  fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
    let builder = WebViewBuilder::new().with_url("https://tauri.app");

    #[cfg(not(target_os = "linux"))]
    {
      let attributes = winit::window::Window::default_attributes()
        .with_title("Reparent (A): Press 'x' to reparent")
        .with_inner_size(winit::dpi::LogicalSize::new(800., 600.));
      let window_a = _event_loop.create_window(attributes).unwrap();

      let attributes2 = winit::window::Window::default_attributes()
        .with_title("Reparent (B): Press 'x' to reparent")
        .with_inner_size(winit::dpi::LogicalSize::new(800., 600.));
      let window_b = _event_loop.create_window(attributes2).unwrap();

      let webview = builder.build(&window_a).unwrap();

      self.window_a = Some(window_a);
      self.window_b = Some(window_b);
      self.webview = Some(webview);
    }

    #[cfg(target_os = "linux")]
    {
      let window_a = gtk::Window::builder()
        .title("Reparent (A): Press 'x' to reparent")
        .default_width(800)
        .default_height(600)
        .build();

      let window_b = gtk::Window::builder()
        .title("Reparent (B): Press 'x' to reparent")
        .default_width(800)
        .default_height(600)
        .build();

      {
        let proxy = self._proxy.clone();
        window_a.connect_close_request(move |_| {
          let _ = proxy.send_event(UserEvent::GtkClosed);
          glib::Propagation::Proceed
        });
      }

      {
        let proxy = self._proxy.clone();
        window_b.connect_close_request(move |_| {
          let _ = proxy.send_event(UserEvent::GtkClosed);
          glib::Propagation::Proceed
        });
      }

      {
        let proxy = self._proxy.clone();
        let controller = gtk::EventControllerKey::new();
        controller.connect_key_pressed(move |_, key, _, _| {
          if key == gdk::Key::x {
            let _ = proxy.send_event(UserEvent::Reparent);
          }
          glib::Propagation::Proceed
        });
        window_a.add_controller(controller);
      }

      {
        let proxy = self._proxy.clone();
        let controller = gtk::EventControllerKey::new();
        controller.connect_key_pressed(move |_, key, _, _| {
          if key == gdk::Key::x {
            let _ = proxy.send_event(UserEvent::Reparent);
          }
          glib::Propagation::Proceed
        });
        window_b.add_controller(controller);
      }

      let webview = builder.build_gtk(&window_a).unwrap();
      window_a.present();
      window_b.present();

      self.window_a = Some(window_a);
      self.window_b = Some(window_b);
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
      #[cfg(not(target_os = "linux"))]
      WindowEvent::KeyboardInput { event, .. } => {
        if event.state == ElementState::Pressed {
          if let Key::Character(c) = &event.logical_key {
            if c.as_str() == "x" {
              let new_parent = if self.on_window_b {
                self.window_a.as_ref().unwrap()
              } else {
                self.window_b.as_ref().unwrap()
              };

              #[cfg(target_os = "macos")]
              {
                use winit::platform::macos::WindowExtMacOS;
                self.on_window_b = !self.on_window_b;
                self
                  .webview
                  .as_ref()
                  .unwrap()
                  .reparent(new_parent.ns_window() as *mut _)
                  .unwrap();
              }
              #[cfg(target_os = "windows")]
              {
                self.on_window_b = !self.on_window_b;
                self
                  .webview
                  .as_ref()
                  .unwrap()
                  .reparent(new_parent.hwnd())
                  .unwrap();
              }
            }
          }
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
      #[cfg(target_os = "linux")]
      UserEvent::Reparent => {
        let new_parent = if self.on_window_b {
          self.window_a.as_ref().unwrap().clone()
        } else {
          self.window_b.as_ref().unwrap().clone()
        };
        self.on_window_b = !self.on_window_b;
        self
          .webview
          .as_mut()
          .unwrap()
          .reparent(&new_parent)
          .unwrap();
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
