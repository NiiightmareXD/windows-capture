"""Fastest Windows Screen Capture Library For Python 🔥."""

from .windows_capture import (
    NativeWindowsCapture,
    NativeCaptureControl,
    NativeDxgiDuplication,
    NativeDxgiDuplicationFrame,
)
import ctypes
import numpy
import cv2
import types
from typing import Optional


class Frame:
    """
    Class To Store A Frame

    ...

    Attributes
    ----------
    frame_buffer : numpy.ndarray
        Raw Buffer Of The Frame
    width : str
        Width Of The Frame
    height : int
        Height Of The Frame
    timespan : int
        Timespan Of The Frame

    Methods
    -------
    save_as_image(path: str):
        Saves The Frame As An Image To The Specified Path
    to_bgr() -> "Frame":
        Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra
    crop(
        start_width : int, start_height : int, end_width : int, end_height : int
    ) -> "Frame":
        Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra
    """

    def __init__(
        self, frame_buffer: numpy.ndarray, width: int, height: int, timespan: int
    ) -> None:
        """Constructs All The Necessary Attributes For The Frame Object"""
        self.frame_buffer = frame_buffer
        self.width = width
        self.height = height
        self.timespan = timespan

    def save_as_image(self, path: str) -> None:
        """Save The Frame As An Image To The Specified Path"""
        cv2.imwrite(path, self.frame_buffer)

    def convert_to_bgr(self) -> "Frame":
        """Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra"""
        bgr_frame_buffer = self.frame_buffer[:, :, :3]

        return Frame(bgr_frame_buffer, self.width, self.height, self.timespan)

    def crop(
        self, start_width: int, start_height: int, end_width: int, end_height: int
    ) -> "Frame":
        """Crops The Frame To The Specified Region"""
        cropped_frame_buffer = self.frame_buffer[
            start_height:end_height, start_width:end_width, :
        ]

        return Frame(
            cropped_frame_buffer,
            end_width - start_width,
            end_height - start_height,
            self.timespan,
        )


class InternalCaptureControl:
    """
    Class To Control The Capturing Session

    ...

    Methods
    -------
    stop():
        Stops The Capture Thread
    """

    def __init__(self, stop_list: list) -> None:
        """Constructs All The Necessary Attributes For The InternalCaptureControl
        Object"""
        self._stop_list: list = stop_list

    def stop(self) -> None:
        """Stops The Capturing Thread"""
        self._stop_list[0] = True


class CaptureControl:
    """
    Class To Control The Capturing Session

    ...

    Methods
    -------
    is_finished():
        Checks To See If Capture Thread Is Finished
    wait():
        Waits Until The Capturing Thread Stops
    stop():
        Gracefully Stop The Capture Thread
    """

    def __init__(self, native_capture_control: NativeCaptureControl) -> None:
        """Constructs All The Necessary Attributes For The CaptureControlObject"""
        self.native_capture_control: NativeCaptureControl = native_capture_control

    def is_finished(self) -> bool:
        """Checks To See If Capture Thread Is Finished"""
        return self.native_capture_control.is_finished()

    def wait(self) -> None:
        """Waits Until The Capturing Thread Stops"""
        self.native_capture_control.wait()

    def stop(self) -> None:
        """Gracefully Stop The Capture Thread"""
        self.native_capture_control.stop()


class WindowsCapture:
    """
    Class To Capture The Screen

    ...

    Attributes
    ----------
    frame_handler : Optional[types.FunctionType]
        The on_frame_arrived Callback Function use @event to Override It Although It Can
        Be Manually Changed
    closed_handler : Optional[types.FunctionType]
        The on_closed Callback Function use @event to Override It Although It Can Be
        Manually Changed

    Methods
    -------
    start():
        Starts The Capture Thread
    start_free_threaded():
        Starts The Capture Thread On A Dedicated Thread
    on_frame_arrived(
        buf : ctypes.POINTER,
        buf_len : int,
        width : int,
        height : int,
        stop_list : list,
    ):
        This Method Is Called Before The on_frame_arrived Callback Function NEVER
        Modify This Method Only Modify The Callback AKA frame_handler
    on_closed():
        This Method Is Called Before The on_closed Callback Function To
        Prepare Data NEVER Modify This Method
        Only Modify The Callback AKA closed_handler
    event(handler: types.FunctionType):
        Overrides The Callback Function
    """

    def __init__(
        self,
        cursor_capture: Optional[bool] = True,
        draw_border: Optional[bool] = None,
        secondary_window: Optional[bool] = None,
        minimum_update_interval: Optional[int] = None,
        dirty_region: Optional[bool] = None,
        monitor_index: Optional[int] = None,
        window_name: Optional[str] = None,
        window_hwnd: Optional[int] = None,
    ) -> None:
        """
        Constructs All The Necessary Attributes For The WindowsCapture Object

        ...

        Parameters
        ----------
            cursor_capture : bool
                Whether To Capture The Cursor
            draw_border : bool
                Whether To draw The border
            secondary_window : bool
                Whether To Capture A Secondary Window
            minimum_update_interval : int
                Minimum Update Interval In Milliseconds
            dirty_region : bool
                Whether To Report And Render Dirty Regions
            monitor_index : int
                Index Of The Monitor To Capture
            window_name : str
                Name Of The Window To Capture (substring match)
            window_hwnd : int
                Window Handle (HWND) To Capture - more reliable than window_name
                for windows with dynamic titles
        """
        # Clear monitor_index if a window target is specified
        if window_name is not None or window_hwnd is not None:
            monitor_index = None

        self.frame_handler: Optional[types.FunctionType] = None
        self.closed_handler: Optional[types.FunctionType] = None
        self.capture = NativeWindowsCapture(
            self.on_frame_arrived,
            self.on_closed,
            cursor_capture,
            draw_border,
            secondary_window,
            minimum_update_interval,
            dirty_region,
            monitor_index,
            window_name,
            window_hwnd,
        )

    def start(self) -> None:
        """Starts The Capture Thread"""
        if self.frame_handler is None:
            raise Exception("on_frame_arrived Event Handler Is Not Set")
        elif self.closed_handler is None:
            raise Exception("on_closed Event Handler Is Not Set")

        self.capture.start()

    def start_free_threaded(self) -> CaptureControl:
        """Starts The Capture Thread On A Dedicated Thread"""
        if self.frame_handler is None:
            raise Exception("on_frame_arrived Event Handler Is Not Set")
        elif self.closed_handler is None:
            raise Exception("on_closed Event Handler Is Not Set")

        native_capture_control = self.capture.start_free_threaded()

        capture_control = CaptureControl(native_capture_control)

        return capture_control

    def on_frame_arrived(
        self,
        buf: ctypes.POINTER,
        buf_len: int,
        width: int,
        height: int,
        stop_list: list,
        timespan: int,
    ) -> None:
        """This Method Is Called Before The on_frame_arrived Callback Function To
        Prepare Data"""
        if self.frame_handler:
            internal_capture_control = InternalCaptureControl(stop_list)

            row_pitch = int(buf_len / height)
            if row_pitch == width * 4:
                ndarray = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, width, 4),
                )

                frame = Frame(ndarray, width, height, timespan)
                self.frame_handler(frame, internal_capture_control)
            else:
                ndarray = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, row_pitch),
                )[:, : width * 4].reshape(height, width, 4)

                frame = Frame(ndarray, width, height, timespan)
                self.frame_handler(frame, internal_capture_control)

                self.frame_handler(
                    frame,
                    internal_capture_control,
                )

        else:
            raise Exception("on_frame_arrived Event Handler Is Not Set")

    def on_closed(self) -> None:
        """This Method Is Called Before The on_closed Callback Function"""
        if self.closed_handler:
            self.closed_handler()
        else:
            raise Exception("on_closed Event Handler Is Not Set")

    def event(self, handler: types.FunctionType) -> types.FunctionType:
        """Overrides The Callback Function"""
        if handler.__name__ == "on_frame_arrived":
            self.frame_handler = handler
        elif handler.__name__ == "on_closed":
            self.closed_handler = handler
        else:
            raise Exception("Invalid Event Handler Use on_frame_arrived Or on_closed")
        return handler


class DxgiDuplicationFrame:
    """Represents a CPU-readable DXGI desktop duplication frame."""

    __slots__ = ("_native", "_numpy_cache")

    def __init__(self, native_frame: NativeDxgiDuplicationFrame) -> None:
        self._native = native_frame
        self._numpy_cache: Optional[numpy.ndarray] = None

    @property
    def width(self) -> int:
        return int(self._native.width)

    @property
    def height(self) -> int:
        return int(self._native.height)

    @property
    def color_format(self) -> str:
        return str(self._native.color_format)

    @property
    def bytes_per_pixel(self) -> int:
        return int(self._native.bytes_per_pixel)

    @property
    def bytes_per_row(self) -> int:
        return int(self._native.bytes_per_row)

    def _raw_buffer(self) -> numpy.ndarray:
        memory_view = self._native.buffer_view()
        raw = numpy.frombuffer(memory_view, dtype=numpy.uint8)
        return raw.reshape(self.height, self.bytes_per_row)

    def to_numpy(self, *, copy: bool = False) -> numpy.ndarray:
        """Returns the frame as a ``numpy.ndarray`` with shape ``(height, width, 4)``.

        The channel order matches the underlying capture format (BGRA or RGBA).
        For ``rgba16f`` frames the returned dtype is ``numpy.float16``; otherwise
        ``numpy.uint8`` is used.
        """

        if self._numpy_cache is not None and not copy:
            return self._numpy_cache

        raw = self._raw_buffer()[:, : self.width * self.bytes_per_pixel]

        if self.color_format == "rgba16f":
            frame = raw.view(numpy.float16).reshape((self.height, self.width, 4))
        else:
            frame = raw.reshape((self.height, self.width, 4))

        if copy:
            return frame.copy()

        self._numpy_cache = frame
        return frame

    def to_bgr(self, *, copy: bool = True) -> numpy.ndarray:
        """Returns the frame converted to BGR ``numpy.uint8`` format."""

        image = self.to_numpy(copy=copy)

        if self.color_format == "bgra8":
            return image[..., :3].copy() if copy else image[..., :3]

        if self.color_format == "rgba8":
            return image[..., 2::-1] if not copy else image[..., [2, 1, 0]].copy()

        # rgba16f -> convert to 0..255 range before casting
        normalized = numpy.clip(image.astype(numpy.float32), 0.0, 1.0)
        return (normalized[..., [2, 1, 0]] * 255.0).astype(numpy.uint8)

    def save_as_image(self, path: str) -> None:
        """Saves the frame to disk using OpenCV."""

        if self.color_format == "rgba16f":
            bgr = self.to_bgr(copy=True)
        else:
            bgr = self.to_bgr(copy=False)

        cv2.imwrite(path, bgr)

    def to_bytes(self) -> bytes:
        """Returns a contiguous copy of the frame bytes."""

        return bytes(self._raw_buffer())


class DxgiDuplicationSession:
    """High-level helper for DXGI desktop duplication captures."""

    __slots__ = ("_native", "_monitor_index")

    def __init__(self, monitor_index: Optional[int] = None) -> None:
        self._native = NativeDxgiDuplication(monitor_index)
        self._monitor_index = monitor_index

    @property
    def monitor_index(self) -> Optional[int]:
        return self._monitor_index

    def acquire_frame(self, timeout_ms: int = 16) -> Optional[DxgiDuplicationFrame]:
        native_frame = self._native.acquire_next_frame(timeout_ms)
        if native_frame is None:
            return None

        return DxgiDuplicationFrame(native_frame)

    def recreate(self) -> None:
        self._native.recreate()

    def switch_monitor(self, monitor_index: int) -> None:
        self._native.switch_monitor(monitor_index)
        self._monitor_index = monitor_index
