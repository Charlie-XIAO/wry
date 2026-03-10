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
use wry::{
  dpi::{LogicalPosition, LogicalSize},
  Rect, WebView, WebViewBuilder,
};

#[derive(Debug)]
enum UserEvent {
  #[cfg(target_os = "linux")]
  GtkClosed,
  #[cfg(target_os = "linux")]
  Resize(i32, i32),
}

struct App {
  #[cfg(not(target_os = "linux"))]
  window: Option<winit::window::Window>,
  #[cfg(target_os = "linux")]
  window: Option<gtk::Window>,
  webviews: Option<[WebView; 4]>,
  proxy: EventLoopProxy<UserEvent>,
}

impl App {
  fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
    Self {
      window: None,
      webviews: None,
      proxy,
    }
  }
}

fn make_bounds(x: u32, y: u32, w: u32, h: u32) -> Rect {
  Rect {
    position: LogicalPosition::new(x, y).into(),
    size: LogicalSize::new(w, h).into(),
  }
}

fn update_bounds(webviews: &[WebView; 4], w: u32, h: u32) {
  webviews[0]
    .set_bounds(make_bounds(0, 0, w / 2, h / 2))
    .unwrap();
  webviews[1]
    .set_bounds(make_bounds(w / 2, 0, w / 2, h / 2))
    .unwrap();
  webviews[2]
    .set_bounds(make_bounds(0, h / 2, w / 2, h / 2))
    .unwrap();
  webviews[3]
    .set_bounds(make_bounds(w / 2, h / 2, w / 2, h / 2))
    .unwrap();
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
          .with_url("https://twitter.com/TauriApps")
          .build(&window)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(w / 2, h / 2, w / 2, h / 2))
          .with_url("https://google.com")
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

      let fixed = ResizableFixed::new();
      window.set_child(Some(&fixed));

      {
        let proxy = self.proxy.clone();
        window.connect_close_request(move |_| {
          let _ = proxy.send_event(UserEvent::GtkClosed);
          glib::Propagation::Proceed
        });
      }

      {
        let proxy = self.proxy.clone();
        fixed.connect_resized(move |_, width, height| {
          println!("(width, height) = ({width}, {height})");
          let _ = proxy.send_event(UserEvent::Resize(width, height));
        });
      }

      let (w, h) = (800u32, 600u32);
      let webviews = [
        WebViewBuilder::new()
          .with_bounds(make_bounds(0, 0, w / 2, h / 2))
          .with_url("https://tauri.app")
          .build_gtk(&fixed)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(w / 2, 0, w / 2, h / 2))
          .with_url("https://github.com/tauri-apps/wry")
          .build_gtk(&fixed)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(0, h / 2, w / 2, h / 2))
          .with_url("https://twitter.com/TauriApps")
          .build_gtk(&fixed)
          .unwrap(),
        WebViewBuilder::new()
          .with_bounds(make_bounds(w / 2, h / 2, w / 2, h / 2))
          .with_url("https://google.com")
          .build_gtk(&fixed)
          .unwrap(),
      ];

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
          update_bounds(webviews, size.width, size.height);
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
      UserEvent::Resize(width, height) => {
        if let Some(webviews) = &self.webviews {
          update_bounds(webviews, width as u32, height as u32);
        }
      }
    }
  }
}

#[cfg(target_os = "linux")]
mod widget_impl {
  use std::{cell::Cell, sync::OnceLock};

  use glib::subclass::Signal;
  use gtk::{glib, prelude::*, subclass::prelude::*};

  #[derive(Default)]
  pub struct ResizableFixed {
    last_w: Cell<i32>,
    last_h: Cell<i32>,
  }

  #[glib::object_subclass]
  impl ObjectSubclass for ResizableFixed {
    const NAME: &str = "ResizableFixed";
    type Type = super::ResizableFixed;
    type ParentType = gtk::Fixed;
  }

  impl ObjectImpl for ResizableFixed {
    fn signals() -> &'static [Signal] {
      static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
      SIGNALS.get_or_init(|| {
        vec![Signal::builder("resized")
          .param_types([i32::static_type(), i32::static_type()])
          .build()]
      })
    }
  }

  impl WidgetImpl for ResizableFixed {
    fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
      self.parent_size_allocate(width, height, baseline);
      if self.last_w.get() == width && self.last_h.get() == height {
        return;
      }
      self.last_w.set(width);
      self.last_h.set(height);
      self.obj().emit_by_name::<()>("resized", &[&width, &height]);
    }
  }

  impl FixedImpl for ResizableFixed {}
}

#[cfg(target_os = "linux")]
glib::wrapper! {
  pub struct ResizableFixed(ObjectSubclass<widget_impl::ResizableFixed>)
    @extends gtk::Fixed, gtk::Widget,
    @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[cfg(target_os = "linux")]
impl ResizableFixed {
  fn new() -> Self {
    glib::Object::new()
  }

  fn connect_resized<F: Fn(&Self, i32, i32) + 'static>(&self, f: F) -> glib::SignalHandlerId {
    self.connect_local("resized", false, move |values| {
      let obj = values[0].get::<ResizableFixed>().unwrap();
      let width = values[1].get::<i32>().unwrap();
      let height = values[2].get::<i32>().unwrap();
      f(&obj, width, height);
      None
    })
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
