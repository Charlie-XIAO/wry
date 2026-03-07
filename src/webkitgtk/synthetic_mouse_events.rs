use std::{cell::RefCell, rc::Rc};

use webkit6::{gdk, gio, gtk, gtk::prelude::*, prelude::*, WebView};

pub fn setup(webview: &WebView) {
  let gesture = gtk::GestureClick::new();
  gesture.set_propagation_phase(gtk::PropagationPhase::Capture);

  let bf_state = BackForwardState(Rc::new(RefCell::new(0)));

  let bf_state_c = bf_state.clone();
  gesture.connect_pressed(move |gesture, n_press, x, y| {
    match gesture.current_button() {
      // back button
      8 => {
        bf_state_c.set(BACK);
        on_gesture_click(gesture, true, n_press, x, y, &bf_state_c);
      }
      // forward button
      9 => {
        bf_state_c.set(FORWARD);
        on_gesture_click(gesture, true, n_press, x, y, &bf_state_c);
      }
      _ => {}
    }
  });

  let bf_state_c = bf_state.clone();
  gesture.connect_released(move |gesture, n_press, x, y| {
    match gesture.current_button() {
      // back button
      8 => {
        bf_state_c.remove(BACK);
        on_gesture_click(gesture, false, n_press, x, y, &bf_state_c);
      }
      // forward button
      9 => {
        bf_state_c.remove(FORWARD);
        on_gesture_click(gesture, false, n_press, x, y, &bf_state_c);
      }
      _ => {}
    }
  });

  webview.add_controller(gesture);
}

fn on_gesture_click(
  gesture: &gtk::GestureClick,
  pressed: bool,
  n_press: i32,
  x: f64,
  y: f64,
  state: &BackForwardState,
) {
  gesture.set_state(gtk::EventSequenceState::Claimed);
  if let Ok(webview) = gesture.widget().and_dynamic_cast::<WebView>() {
    webview.evaluate_javascript(
      &create_js_mouse_event(gesture, n_press, x, y, pressed, state),
      None,
      None,
      gio::Cancellable::NONE,
      |_| {},
    )
  }
}

fn create_js_mouse_event(
  gesture: &gtk::GestureClick,
  n_press: i32,
  x: f64,
  y: f64,
  pressed: bool,
  state: &BackForwardState,
) -> String {
  let modifier_state = gesture.current_event_state();
  let event_name = if pressed { "mousedown" } else { "mouseup" };

  // js equivalent https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/button
  let button = match gesture.current_button() {
    8 => 3,
    9 => 4,
    _ => 0,
  };

  let x = x.round() as i32;
  let y = y.round() as i32;

  let mut buttons = 0;
  // left button
  if modifier_state.contains(gdk::ModifierType::BUTTON1_MASK) {
    buttons |= 1;
  }
  // right button
  if modifier_state.contains(gdk::ModifierType::BUTTON3_MASK) {
    buttons |= 2;
  }
  // middle button
  if modifier_state.contains(gdk::ModifierType::BUTTON2_MASK) {
    buttons |= 4;
  }
  // back button
  if state.has(BACK) {
    buttons |= 8;
  }
  // forward button
  if state.has(FORWARD) {
    buttons |= 16;
  }

  format!(
    r#"(() => {{
      const el = document.elementFromPoint({x},{y});
      if (!el) return;
      const ev = new MouseEvent('{event_name}', {{
        view: window,
        button: {button},
        buttons: {buttons},
        x: {x},
        y: {y},
        bubbles: true,
        detail: {detail},
        cancelable: true,
        clientX: {x},
        clientY: {y},
        pageX: {x},
        pageY: {y},
        screenX: window.screenX + {x},
        screenY: window.screenY + {y},
        ctrlKey: {ctrl_key},
        metaKey: {meta_key},
        shiftKey: {shift_key},
        altKey: {alt_key},
      }});
      el.dispatchEvent(ev);
      if (!ev.defaultPrevented && "{event_name}" === "mouseup") {{
        if (ev.button === 3) window.history.back();
        if (ev.button === 4) window.history.forward();
      }}
    }})()"#,
    event_name = event_name,
    x = x,
    y = y,
    detail = n_press.max(1),
    ctrl_key = modifier_state.contains(gdk::ModifierType::CONTROL_MASK),
    alt_key = modifier_state.contains(gdk::ModifierType::ALT_MASK),
    shift_key = modifier_state.contains(gdk::ModifierType::SHIFT_MASK),
    meta_key = modifier_state.contains(gdk::ModifierType::SUPER_MASK),
    button = button,
    buttons = buttons,
  )
}

// Internal modifiers to track whether BACK/FORWARD buttons are pressed
const BACK: u8 = 0b01;
const FORWARD: u8 = 0b10;

/// A single u8 that stores whether [BACK] and [FORWARD] are pressed or not
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BackForwardState(Rc<RefCell<u8>>);

impl BackForwardState {
  fn set(&self, button: u8) {
    *self.0.borrow_mut() |= button
  }

  fn remove(&self, button: u8) {
    *self.0.borrow_mut() &= !button
  }

  fn has(&self, button: u8) -> bool {
    let state = *self.0.borrow();
    state & !button != state
  }
}
