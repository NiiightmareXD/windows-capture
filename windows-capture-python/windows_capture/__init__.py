"""Fastest Windows Screen Capture Library For Python 🔥."""

from .windows_capture import NativeWindowsCapture, NativeCaptureControl
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

    def __init__(self, frame_buffer: numpy.ndarray, width: int, height: int, timespan: int) -> None:
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
            cropped_frame_buffer, end_width - start_width, end_height - start_height, self.timespan
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
        monitor_index: Optional[int] = None,
        window_name: Optional[str] = None,
        window_handle: Optional[int] = None,
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
            monitor_index : int
                Index Of The Monitor To Capture
            window_name : str
                Name Of The Window To Capture
            window_handle : int
                Handle Of The Window To Capture
        """
        specified_count = sum(
            param is not None for param in [window_name, monitor_index, window_handle]
        )

        if specified_count > 1:
            raise ValueError(
                "You can specify only one of window_name, monitor_index, or window_handle."
            )

        self.frame_handler: Optional[types.FunctionType] = None
        self.closed_handler: Optional[types.FunctionType] = None
        self.capture = NativeWindowsCapture(
            self.on_frame_arrived,
            self.on_closed,
            cursor_capture,
            draw_border,
            monitor_index,
            window_name,
            window_handle,
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
