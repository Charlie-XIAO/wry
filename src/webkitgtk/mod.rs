// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use dpi::LogicalSize;
use gtk::{gdk, gio, glib, prelude::*};
use http::Request;
use raw_window_handle::HasWindowHandle;
#[cfg(any(debug_assertions, feature = "devtools"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
  collections::HashMap,
  rc::Rc,
  sync::{Arc, Mutex},
};
use webkit::{
  prelude::*, soup, AutoplayPolicy, LoadEvent, NavigationPolicyDecision, NetworkProxyMode,
  NetworkProxySettings, PolicyDecisionType, PrintOperation, URIRequest, UserContentInjectedFrames,
  UserContentManager, UserScript, UserScriptInjectionTime, WebView, WebsiteDataTypes,
  WebsitePolicies,
};

use web_context::WebContextExt;
pub use web_context::WebContextImpl;

use crate::{
  proxy::ProxyConfig, web_context::WebContext, Error, NewWindowFeatures, NewWindowOpener,
  NewWindowResponse, PageLoadEvent, Rect, Result, WebViewAttributes, RGBA,
};

const WEBVIEW_ID: &str = "webview_id";

mod drag_drop;
mod synthetic_mouse_events;
mod web_context;

pub(crate) struct InnerWebView {
  id: String,
  pub webview: WebView,
  #[cfg(any(debug_assertions, feature = "devtools"))]
  is_inspector_open: Arc<AtomicBool>,
  pending_scripts: Arc<Mutex<Option<Vec<String>>>>,
}

impl Drop for InnerWebView {
  fn drop(&mut self) {
    Self::remove_from_parent(&self.webview);
  }
}

impl InnerWebView {
  pub fn new(
    _window: &impl HasWindowHandle,
    _attributes: WebViewAttributes,
    _pl_attrs: super::PlatformSpecificWebViewAttributes,
  ) -> Result<Self> {
    Err(Error::UnsupportedWindowHandle)
  }

  pub fn new_as_child(
    _window: &impl HasWindowHandle,
    _attributes: WebViewAttributes,
    _pl_attrs: super::PlatformSpecificWebViewAttributes,
  ) -> Result<Self> {
    Err(Error::UnsupportedWindowHandle)
  }

  pub fn new_gtk<W>(
    container: &W,
    mut attributes: WebViewAttributes,
    pl_attrs: super::PlatformSpecificWebViewAttributes,
  ) -> Result<Self>
  where
    W: IsA<gtk::Widget>,
  {
    // default_context allows us to create a scoped context on-demand
    let mut default_context;
    let web_context = if attributes.incognito {
      default_context = WebContext::new_ephemeral();
      &mut default_context
    } else {
      match attributes.context.take() {
        Some(w) => w,
        None => {
          default_context = Default::default();
          &mut default_context
        }
      }
    };
    if let Some(proxy_setting) = &attributes.proxy_config {
      let proxy_uri = match proxy_setting {
        ProxyConfig::Http(endpoint) => format!("http://{}:{}", endpoint.host, endpoint.port),
        ProxyConfig::Socks5(endpoint) => {
          format!("socks5://{}:{}", endpoint.host, endpoint.port)
        }
      };
      let network_session = web_context.network_session();
      let settings = NetworkProxySettings::new(Some(proxy_uri.as_str()), &[]);
      network_session.set_proxy_settings(NetworkProxyMode::Custom, Some(&settings));
    }

    // Extension loading
    if let Some(extension_path) = &pl_attrs.extension_path {
      web_context.os.set_web_extensions_directory(extension_path);
    }

    let webview = Self::create_webview(web_context, &attributes, &pl_attrs);

    // Transparent
    if attributes.transparent {
      webview.set_background_color(&gdk::RGBA::new(0., 0., 0., 0.));
    } else {
      // background color
      if let Some((red, green, blue, alpha)) = attributes.background_color {
        webview.set_background_color(&gdk::RGBA::new(red as _, green as _, blue as _, alpha as _));
      }
    }

    // Webview Settings
    Self::set_webview_settings(&webview, &attributes);

    // Webview handlers
    Self::attach_handlers(&webview, web_context, &mut attributes);

    // IPC handler
    Self::attach_ipc_handler(webview.clone(), &mut attributes);

    // Drag drop handler
    if let Some(drag_drop_handler) = attributes.drag_drop_handler.take() {
      drag_drop::connect_drag_event(&webview, drag_drop_handler);
    }

    web_context.register_automation(webview.clone());

    Self::add_to_container(&webview, container)?;

    #[cfg(any(debug_assertions, feature = "devtools"))]
    let is_inspector_open = Self::attach_inspector_handlers(&webview);

    let id = attributes
      .id
      .map(|id| id.to_string())
      .unwrap_or_else(|| (webview.as_ptr() as isize).to_string());
    unsafe { webview.set_data(WEBVIEW_ID, id.clone()) };

    let w = Self {
      id,
      webview,
      pending_scripts: Arc::new(Mutex::new(Some(Vec::new()))),
      #[cfg(any(debug_assertions, feature = "devtools"))]
      is_inspector_open,
    };

    // Initialize message handler
    w.init("Object.defineProperty(window, 'ipc', { value: Object.freeze({ postMessage: function(x) { window.webkit.messageHandlers['ipc'].postMessage(x) } }) })", true)?;

    // Initialize scripts
    for init_script in attributes.initialization_scripts {
      w.init(&init_script.script, init_script.for_main_frame_only)?;
    }

    // Run pending webview.eval() scripts once webview loads.
    let pending_scripts = w.pending_scripts.clone();
    w.webview.connect_load_changed(move |webview, event| {
      if let LoadEvent::Committed = event {
        let mut pending_scripts_ = pending_scripts.lock().unwrap();
        if let Some(pending_scripts) = pending_scripts_.take() {
          for script in pending_scripts {
            webview.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| ());
          }
        }
      }
    });

    // Custom protocols handler
    for (name, handler) in attributes.custom_protocols {
      web_context.register_uri_scheme(&name, handler)?;
    }

    // Navigation
    if let Some(url) = attributes.url {
      web_context.load_uri(w.webview.clone(), url, attributes.headers);
    } else if let Some(html) = attributes.html {
      w.webview.load_html(&html, None);
    }

    w.webview.set_visible(attributes.visible);

    if attributes.focused {
      w.webview.grab_focus();
    }

    Ok(w)
  }

  fn create_webview(
    web_context: &WebContext,
    attributes: &WebViewAttributes,
    pl_attrs: &super::PlatformSpecificWebViewAttributes,
  ) -> WebView {
    let mut builder = WebView::builder()
      .user_content_manager(&UserContentManager::new())
      .is_controlled_by_automation(web_context.allows_automation())
      .network_session(web_context.network_session());

    if attributes.autoplay {
      builder = builder.website_policies(
        &WebsitePolicies::builder()
          .autoplay(AutoplayPolicy::Allow)
          .build(),
      );
    }

    if let Some(related_view) = &pl_attrs.related_view {
      builder = builder.related_view(related_view);
    } else {
      builder = builder.web_context(web_context.context());
    }

    builder.build()
  }

  fn set_webview_settings(webview: &WebView, attributes: &WebViewAttributes) {
    // Disable input preedit,fcitx input editor can anchor at edit cursor position
    if let Some(input_context) = webview.input_method_context() {
      input_context.set_enable_preedit(false);
    }

    if let Some(settings) = WebViewExt::settings(webview) {
      // Enable webgl, webaudio, canvas features as default.
      settings.set_enable_webgl(true);
      settings.set_enable_webaudio(true);
      settings
        .set_enable_back_forward_navigation_gestures(attributes.back_forward_navigation_gestures);

      // Enable clipboard
      if attributes.clipboard {
        settings.set_javascript_can_access_clipboard(true);
      }

      // Enable App cache
      settings.set_enable_page_cache(true);

      // Set user agent
      settings.set_user_agent(attributes.user_agent.as_deref());

      // Devtools
      if attributes.devtools {
        settings.set_enable_developer_extras(true);
      }

      if attributes.javascript_disabled {
        settings.set_enable_javascript(false);
      }
    }
  }

  fn attach_handlers(
    webview: &WebView,
    web_context: &mut WebContext,
    attributes: &mut WebViewAttributes,
  ) {
    // window.close()
    webview.connect_close(move |webview| {
      Self::remove_from_parent(webview);
    });

    // Synthetic mouse events
    synthetic_mouse_events::setup(webview);

    // Document title changed handler
    if let Some(document_title_changed_handler) = attributes.document_title_changed_handler.take() {
      webview.connect_title_notify(move |webview| {
        let new_title = webview.title().map(|t| t.to_string()).unwrap_or_default();
        document_title_changed_handler(new_title)
      });
    }

    // Page load handler
    if let Some(on_page_load_handler) = attributes.on_page_load_handler.take() {
      webview.connect_load_changed(move |webview, load_event| match load_event {
        LoadEvent::Committed => {
          on_page_load_handler(PageLoadEvent::Started, webview.uri().unwrap().to_string());
        }
        LoadEvent::Finished => {
          on_page_load_handler(PageLoadEvent::Finished, webview.uri().unwrap().to_string());
        }
        _ => (),
      });
    }

    // window creation handler
    if let Some(new_window_req_handler) = attributes.new_window_req_handler.take() {
      let related_webviews = Rc::new(Mutex::new(HashMap::new()));
      webview.connect_create(move |webview, action| {
        let url = action
          .request()
          .and_then(|request| request.uri())
          .map(|uri| uri.as_str().to_string())?;
        match new_window_req_handler(
          url.clone(),
          NewWindowFeatures {
            size: None,
            position: None,
            opener: NewWindowOpener {
              webview: webview.clone(),
            },
          },
        ) {
          NewWindowResponse::Allow => {
            let related_webviews = related_webviews.clone();
            let window = webview.root()?.downcast::<gtk::ApplicationWindow>().ok()?;
            let id = window.id();
            let app = window.application()?;

            let window = gtk::ApplicationWindow::builder()
              .application(&app)
              .title(&url)
              .build();
            let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
            window.set_child(Some(&box_));

            let related_webviews_ = related_webviews.clone();
            window.connect_destroy(move |_| {
              related_webviews_.lock().unwrap().remove(&id);
            });

            window.present();
            Self::new_gtk(
              &box_,
              WebViewAttributes::default(),
              super::PlatformSpecificWebViewAttributes {
                related_view: Some(webview.clone()),
                ..Default::default()
              },
            )
            .map(|webview| {
              let widget = webview.webview.upcast_ref::<gtk::Widget>().clone();
              related_webviews.lock().unwrap().insert(id, webview);
              widget
            })
            .ok()
          }
          NewWindowResponse::Create { webview } => Some(webview.upcast::<gtk::Widget>()),
          NewWindowResponse::Deny => None,
        }
      });
    }

    // Navigation handler
    if let Some(navigation_handler) = attributes.navigation_handler.take() {
      webview.connect_decide_policy(move |_webview, policy_decision, policy_type| {
        let handler = match policy_type {
          PolicyDecisionType::NavigationAction => &navigation_handler,
          _ => return false,
        };

        if let Some(policy) = policy_decision.dynamic_cast_ref::<NavigationPolicyDecision>() {
          if let Some(nav_action) = policy.navigation_action() {
            if let Some(uri_req) = nav_action.request() {
              if let Some(uri) = uri_req.uri() {
                let allow = handler(uri.to_string());
                if allow {
                  policy_decision.use_();
                } else {
                  policy_decision.ignore();
                }
                return true;
              }
            }
          }
        }

        false
      });
    }

    // Download handler
    if attributes.download_started_handler.is_some()
      || attributes.download_completed_handler.is_some()
    {
      web_context.register_download_handler(
        attributes.download_started_handler.take(),
        attributes.download_completed_handler.take(),
      )
    }
  }

  fn add_to_container<W>(webview: &WebView, container: &W) -> Result<()>
  where
    W: IsA<gtk::Widget>,
  {
    if let Some(c) = container.dynamic_cast_ref::<gtk::Window>() {
      c.set_child(Some(webview));
    } else if let Some(c) = container.dynamic_cast_ref::<gtk::Box>() {
      c.append(webview);
      webview.set_hexpand(true);
      webview.set_vexpand(true);
    } else {
      return Err(Error::UnsupportedParentWidget(
        container.type_().name().to_string(),
      ));
    }

    Ok(())
  }

  fn remove_from_parent(webview: &WebView) {
    if let Some(parent) = webview.parent() {
      if let Some(p) = parent.dynamic_cast_ref::<gtk::Window>() {
        p.set_child(gtk::Widget::NONE);
      } else if let Some(p) = parent.dynamic_cast_ref::<gtk::Box>() {
        p.remove(webview);
      }
    }
  }

  fn attach_ipc_handler(webview: WebView, attributes: &mut WebViewAttributes) {
    // Message handler
    let ipc_handler = attributes.ipc_handler.take();
    let manager = webview
      .user_content_manager()
      .expect("WebView does not have UserContentManager");

    // Connect before registering as recommended by the docs
    manager.connect_script_message_received(None, move |_m, msg| {
      #[cfg(feature = "tracing")]
      let _span = tracing::info_span!(parent: None, "wry::ipc::handle").entered();

      if let Some(ipc_handler) = &ipc_handler {
        ipc_handler(
          Request::builder()
            .uri(webview.uri().unwrap().to_string())
            .body(msg.to_string())
            .unwrap(),
        );
      }
    });

    // Register the handler we just connected
    manager.register_script_message_handler("ipc", None);
  }

  #[cfg(any(debug_assertions, feature = "devtools"))]
  fn attach_inspector_handlers(webview: &WebView) -> Arc<AtomicBool> {
    let is_inspector_open = Arc::new(AtomicBool::default());
    if let Some(inspector) = webview.inspector() {
      let is_inspector_open_ = is_inspector_open.clone();
      inspector.connect_bring_to_front(move |_| {
        is_inspector_open_.store(true, Ordering::Relaxed);
        false
      });
      let is_inspector_open_ = is_inspector_open.clone();
      inspector.connect_closed(move |_| {
        is_inspector_open_.store(false, Ordering::Relaxed);
      });
    }
    is_inspector_open
  }

  pub fn id(&self) -> crate::WebViewId<'_> {
    &self.id
  }

  pub fn print(&self) -> Result<()> {
    let print = PrintOperation::new(&self.webview);
    print.run_dialog(None::<&gtk::Window>);
    Ok(())
  }

  pub fn url(&self) -> Result<String> {
    Ok(self.webview.uri().unwrap_or_default().to_string())
  }

  pub fn eval(
    &self,
    js: &str,
    callback: Option<impl FnOnce(String) + Send + 'static>,
  ) -> Result<()> {
    if let Some(pending_scripts) = &mut *self.pending_scripts.lock().unwrap() {
      pending_scripts.push(js.into());
    } else {
      #[cfg(feature = "tracing")]
      let span = SendEnteredSpan(tracing::debug_span!("wry::eval").entered());

      self
        .webview
        .evaluate_javascript(js, None, None, gio::Cancellable::NONE, |result| {
          #[cfg(feature = "tracing")]
          drop(span);

          if let Some(callback) = callback {
            let result = result
              .map(|r| r.to_json(0))
              .unwrap_or_default()
              .unwrap_or_default()
              .to_string();

            callback(result);
          }
        });
    }

    Ok(())
  }

  fn init(&self, js: &str, for_main_only: bool) -> Result<()> {
    if let Some(manager) = self.webview.user_content_manager() {
      let script = UserScript::new(
        js,
        if for_main_only {
          UserContentInjectedFrames::TopFrame
        } else {
          UserContentInjectedFrames::AllFrames
        },
        UserScriptInjectionTime::Start,
        &[],
        &[],
      );
      manager.add_script(&script);
    } else {
      return Err(Error::InitScriptError);
    }
    Ok(())
  }

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn open_devtools(&self) {
    if let Some(inspector) = self.webview.inspector() {
      inspector.show();
      // `bring-to-front` is not received in this case
      self.is_inspector_open.store(true, Ordering::Relaxed);
    }
  }

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn close_devtools(&self) {
    if let Some(inspector) = self.webview.inspector() {
      inspector.close();
    }
  }

  #[cfg(any(debug_assertions, feature = "devtools"))]
  pub fn is_devtools_open(&self) -> bool {
    self.is_inspector_open.load(Ordering::Relaxed)
  }

  pub fn zoom(&self, scale_factor: f64) -> Result<()> {
    self.webview.set_zoom_level(scale_factor);
    Ok(())
  }

  pub fn set_background_color(&self, (red, green, blue, alpha): RGBA) -> Result<()> {
    self
      .webview
      .set_background_color(&gdk::RGBA::new(red as _, green as _, blue as _, alpha as _));
    Ok(())
  }

  pub fn load_url(&self, url: &str) -> Result<()> {
    self.webview.load_uri(url);
    Ok(())
  }

  pub fn load_url_with_headers(&self, url: &str, headers: http::HeaderMap) -> Result<()> {
    let req = URIRequest::new(url);

    if let Some(req_headers) = req.http_headers() {
      for (header, value) in headers.iter() {
        req_headers.append(
          header.to_string().as_str(),
          value.to_str().unwrap_or_default(),
        );
      }
    }

    self.webview.load_request(&req);

    Ok(())
  }

  pub fn load_html(&self, html: &str) -> Result<()> {
    self.webview.load_html(html, None);
    Ok(())
  }

  pub fn reload(&self) -> Result<()> {
    self.webview.reload();
    Ok(())
  }

  pub fn clear_all_browsing_data(&self) -> Result<()> {
    if let Some(network_session) = self.webview.network_session() {
      if let Some(data_manger) = network_session.website_data_manager() {
        data_manger.clear(
          WebsiteDataTypes::ALL,
          glib::TimeSpan::from_seconds(0),
          gio::Cancellable::NONE,
          |_| {},
        );
      }
    }

    Ok(())
  }

  pub fn bounds(&self) -> Result<Rect> {
    Ok(Rect {
      size: LogicalSize::new(self.webview.width(), self.webview.height()).into(),
      ..Default::default()
    })
  }

  pub fn set_bounds(&self, _bounds: Rect) -> Result<()> {
    Ok(()) // Not supported
  }

  pub fn set_visible(&self, visible: bool) -> Result<()> {
    self.webview.set_visible(visible);
    Ok(())
  }

  pub fn focus(&self) -> Result<()> {
    self.webview.grab_focus();
    Ok(())
  }

  pub fn focus_parent(&self) -> Result<()> {
    if let Some(root) = self.webview.root() {
      if let Ok(window) = root.downcast::<gtk::Window>() {
        window.grab_focus();
      }
    }

    Ok(())
  }

  fn cookie_from_soup_cookie(mut cookie: soup::Cookie) -> cookie::Cookie<'static> {
    let name = cookie.name().map(|n| n.to_string()).unwrap_or_default();
    let value = cookie.value().map(|n| n.to_string()).unwrap_or_default();

    let mut cookie_builder = cookie::CookieBuilder::new(name, value);

    if let Some(domain) = cookie.domain().map(|n| n.to_string()) {
      cookie_builder = cookie_builder.domain(domain);
    }

    if let Some(path) = cookie.path().map(|n| n.to_string()) {
      cookie_builder = cookie_builder.path(path);
    }

    let http_only = cookie.is_http_only();
    cookie_builder = cookie_builder.http_only(http_only);

    let secure = cookie.is_secure();
    cookie_builder = cookie_builder.secure(secure);

    let same_site = cookie.same_site_policy();
    let same_site = match same_site {
      soup::SameSitePolicy::Lax => cookie::SameSite::Lax,
      soup::SameSitePolicy::Strict => cookie::SameSite::Strict,
      soup::SameSitePolicy::None => cookie::SameSite::None,
      _ => cookie::SameSite::None,
    };
    cookie_builder = cookie_builder.same_site(same_site);

    let expires = cookie.expires();
    let expires = match expires {
      Some(datetime) => cookie::time::OffsetDateTime::from_unix_timestamp(datetime.to_unix())
        .ok()
        .map(cookie::Expiration::DateTime),
      None => Some(cookie::Expiration::Session),
    };
    if let Some(expires) = expires {
      cookie_builder = cookie_builder.expires(expires);
    }

    cookie_builder.build()
  }

  fn cookie_into_soup_cookie(cookie: &cookie::Cookie<'_>) -> soup::Cookie {
    let mut soup_cookie = soup::Cookie::new(
      cookie.name(),
      cookie.value(),
      cookie.domain().unwrap_or(""),
      cookie.path().unwrap_or(""),
      cookie
        .max_age()
        .map(|d| d.whole_seconds() as i32)
        .unwrap_or(-1),
    );

    if let Some(dt) = cookie.expires_datetime() {
      soup_cookie.set_expires(&glib::DateTime::from_unix_utc(dt.unix_timestamp()).unwrap());
    }

    if let Some(http_only) = cookie.http_only() {
      soup_cookie.set_http_only(http_only);
    }

    if let Some(same_site) = cookie.same_site() {
      soup_cookie.set_same_site_policy(match same_site {
        cookie::SameSite::Lax => soup::SameSitePolicy::Lax,
        cookie::SameSite::Strict => soup::SameSitePolicy::Strict,
        cookie::SameSite::None => soup::SameSitePolicy::None,
      });
    }

    if let Some(secure) = cookie.secure() {
      soup_cookie.set_secure(secure);
    }

    soup_cookie
  }

  pub fn cookies_for_url(&self, url: &str) -> Result<Vec<cookie::Cookie<'static>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(cookies_manager) = self
      .webview
      .network_session()
      .and_then(|session| session.cookie_manager())
    {
      cookies_manager.cookies(url, gio::Cancellable::NONE, move |cookies| {
        let cookies = cookies.map(|cookies| {
          cookies
            .into_iter()
            .map(Self::cookie_from_soup_cookie)
            .collect()
        });
        let _ = tx.send(cookies);
      })
    }

    let context = glib::MainContext::default();
    loop {
      context.iteration(true);
      if let Ok(response) = rx.try_recv() {
        return response.map_err(Into::into);
      }
    }
  }

  pub fn cookies(&self) -> Result<Vec<cookie::Cookie<'static>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(cookies_manager) = self
      .webview
      .network_session()
      .and_then(|session| session.cookie_manager())
    {
      cookies_manager.all_cookies(gio::Cancellable::NONE, move |cookies| {
        let cookies = cookies.map(|cookies| {
          cookies
            .into_iter()
            .map(Self::cookie_from_soup_cookie)
            .collect()
        });
        let _ = tx.send(cookies);
      })
    }

    let context = glib::MainContext::default();
    loop {
      context.iteration(true);
      if let Ok(response) = rx.try_recv() {
        return response.map_err(Into::into);
      }
    }
  }

  pub fn set_cookie(&self, cookie: &cookie::Cookie<'_>) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(cookies_manager) = self
      .webview
      .network_session()
      .and_then(|session| session.cookie_manager())
    {
      let mut soup_cookie = Self::cookie_into_soup_cookie(cookie);
      cookies_manager.add_cookie(&mut soup_cookie, gio::Cancellable::NONE, move |ret| {
        let _ = tx.send(ret);
      });
    }

    let context = glib::MainContext::default();
    loop {
      context.iteration(true);
      if let Ok(response) = rx.try_recv() {
        return response.map_err(Into::into);
      }
    }
  }

  pub fn delete_cookie(&self, cookie: &cookie::Cookie<'_>) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(cookies_manager) = self
      .webview
      .network_session()
      .and_then(|session| session.cookie_manager())
    {
      let mut soup_cookie = Self::cookie_into_soup_cookie(cookie);
      cookies_manager.delete_cookie(&mut soup_cookie, gio::Cancellable::NONE, move |ret| {
        let _ = tx.send(ret);
      });
    }

    let context = glib::MainContext::default();
    loop {
      context.iteration(true);
      if let Ok(response) = rx.try_recv() {
        return response.map_err(Into::into);
      }
    }
  }

  pub fn reparent<W>(&mut self, container: &W) -> Result<()>
  where
    W: IsA<gtk::Widget>,
  {
    Self::remove_from_parent(&self.webview);
    Self::add_to_container(&self.webview, container)?;
    Ok(())
  }
}

pub fn platform_webview_version() -> Result<String> {
  let (major, minor, patch) = (
    webkit::functions::major_version(),
    webkit::functions::minor_version(),
    webkit::functions::micro_version(),
  );
  Ok(format!("{major}.{minor}.{patch}"))
}

// SAFETY: only use this when you are sure the span will be dropped on the same thread it was entered
#[cfg(feature = "tracing")]
struct SendEnteredSpan(#[allow(dead_code)] tracing::span::EnteredSpan);

#[cfg(feature = "tracing")]
unsafe impl Send for SendEnteredSpan {}
