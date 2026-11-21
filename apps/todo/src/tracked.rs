//! Records a widget's laid-out bounds without adding a layout node.
//!
//! Virtual clicks aim at the centre of the recorded bounds, so they must be
//! the bounds GPUI hit-tests. A wrapper `div` with a `canvas` child records
//! the wrapper instead: the wrapper takes the widget's place in its parent's
//! flex layout (a `flex_1` input shrank to its content) and the absolute
//! canvas lands off the widget. This element hands the child's own
//! [`LayoutId`] to the parent, so layout is unchanged and the bounds GPUI
//! passes to `prepaint` are exactly the child's.

use std::panic::Location;

use gpui_kit::*;

type OnBounds = Box<dyn FnOnce(Bounds<Pixels>, &mut App)>;

/// Transparent element that reports its child's bounds during prepaint.
pub(crate) struct Tracked {
    child: AnyElement,
    on_bounds: Option<OnBounds>,
}

/// Wraps `child` so `on_bounds` receives the child's window-space bounds
/// (scroll offset applied, the space hit-tests use) once per frame.
pub(crate) fn tracked(
    child: impl IntoElement,
    on_bounds: impl FnOnce(Bounds<Pixels>, &mut App) + 'static,
) -> Tracked {
    Tracked {
        child: child.into_any_element(),
        on_bounds: Some(Box::new(on_bounds)),
    }
}

impl IntoElement for Tracked {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Tracked {
    type RequestLayoutState = ();
    type PrepaintState = ();

    // No id: the wrapper must not take element state or hitboxes from the
    // widget that virtual clicks have to reach.
    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(on_bounds) = self.on_bounds.take() {
            on_bounds(bounds, cx);
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
