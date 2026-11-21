"""The compose adapter: frames put together by `bullterrier_scene`.

Each call builds a small scene of picture-hole layers (rebuilding one costs microseconds), fills the holes with the
frames and renders it. The core hands over layers as plain data (a frame, a view, an opacity, a blend name, a mask)
and never learns scene's names; `bullterrier_scene` is imported when the adapter is made."""

from __future__ import annotations

import importlib
from typing import TYPE_CHECKING, Any

from bullterrier_video.errors import PortError

if TYPE_CHECKING:
    from collections.abc import Sequence

    import numpy as np
    from numpy.typing import NDArray

__all__ = ["SceneCompose"]

_IDENTITY = (0.0, 0.0, 1.0, 0.0)

Layer = tuple[
    "NDArray[np.float32]", tuple[float, float, float, float], float, str, "NDArray[np.float32] | None"
]  # frame, view (dx, dy, zoom, rotate), opacity, blend, mask


class SceneCompose:
    """Layers into one frame through scene: the placement, opacity, blend and mask of each."""

    def __init__(self) -> None:
        """Imports scene, or raises a `PortError` that says how to get it."""
        try:
            self._scene = importlib.import_module(
                "bullterrier_scene"
            )  # when a camera or transition is used: video needs no scene to import
        except ImportError as failure:
            message = (
                "compose: bullterrier_scene is not installed; install bullterrier-scene to use a camera or a transition"
            )
            raise PortError(message) from failure

    def compose(self, layers: Sequence[Layer]) -> NDArray[np.float32]:
        """The layers over one another, the first at the bottom, as a float32 `(height, width, 4)` frame.

        The result has the size of the first frame. A view moves a layer by `dx`, `dy` (fractions of the long side),
        `zoom` and `rotate` (degrees clockwise) about the centre.
        """
        height, width = layers[0][0].shape[:2]
        long_side = max(width, height)
        scene = self._scene.Scene(width, height)
        values: dict[str, Any] = {}
        for index, (frame, view, opacity, blend, mask) in enumerate(layers):
            name = f"frame{index}"
            layer = self._scene.Layer.picture(None, maps=None if mask is None else {"mask": None}).named(name)
            if view != _IDENTITY:
                dx, dy, zoom, rotate = view
                layer = layer.place(dx=dx * long_side, dy=dy * long_side, scale=zoom, rotate=rotate)
            if mask is not None:
                layer = layer.mask("mask")
                values[f"{name}.map.mask"] = mask
            if opacity != 1.0:
                layer = layer.opacity(opacity)
            if blend != "normal":
                layer = layer.blend(blend)
            scene = scene.add(layer)
            values[f"{name}.picture"] = frame
        return scene.with_values(values).render(long_side)
