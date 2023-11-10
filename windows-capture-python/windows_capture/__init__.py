"""Fastest Windows Screen Capture Library For Python 🔥."""

from .windows_capture import NativeWindowsCapture
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

    Methods
    -------
    save_as_image(path: str):
        Saves The Frame As An Image To Specified Path
    to_bgr() -> "Frame":
        Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra
    crop(
        start_width : int, start_height : int, end_width : int, end_height : int
    ) -> "Frame":
        Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra
    """

    def __init__(self, frame_buffer: numpy.ndarray, width: int, height: int) -> None:
        """Constructs All The Necessary Attributes For The Frame Object"""
        self.frame_buffer = frame_buffer
        self.width = width
        self.height = height

    def save_as_image(self, path: str) -> None:
        """Save The Frame As An Image To Specified Path"""
        cv2.imwrite(path, self.frame_buffer)

    def convert_to_bgr(self) -> "Frame":
        """Converts The self.frame_buffer Pixel Type To Bgr Instead Of Bgra"""
        bgr_frame_buffer = self.frame_buffer[:, :, :3]

        return Frame(bgr_frame_buffer, self.width, self.height)

    def crop(
        self, start_width: int, start_height: int, end_width: int, end_height: int
    ) -> "Frame":
        """Crops The Frame To The Specified Region"""
        cropped_frame_buffer = self.frame_buffer[
            start_height:end_height, start_width:end_width, :
        ]

        return Frame(
            cropped_frame_buffer, end_width - start_width, end_height - start_height
        )


class CaptureControl:
    """
    Class To Control The Capturing Session

    ...

    Methods
    -------
    stop():
        Stops The Capture Thread
    """

    def __init__(self, list: list) -> None:
        """Constructs All The Necessary Attributes For The CaptureControl Object"""
        self._list = list

    def stop(self) -> None:
        """Stops The Capturing Thread"""
        self._list[0] = True


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
        capture_cursor: bool = True,
        draw_border: bool = False,
        monitor_index: int = 0,
        window_name: Optional[str] = None,
    ) -> None:
        """
        Constructs All The Necessary Attributes For The WindowsCapture Object

        ...

        Parameters
        ----------
            capture_cursor : bool
                Whether To Capture The Cursor
            draw_border : bool
                Whether To draw The border
            monitor_index : int
                Index Of The Monitor To Capture
            window_name : str
                Name Of The Window To Capture
        """
        if window_name is not None:
            monitor_index = None

        self.frame_handler: Optional[types.FunctionType] = None
        self.closed_handler: Optional[types.FunctionType] = None
        self.capture = NativeWindowsCapture(
            self.on_frame_arrived,
            self.on_closed,
            capture_cursor,
            draw_border,
            monitor_index,
            window_name,
        )

    def start(self) -> None:
        """Starts The Capture Thread"""
        if self.frame_handler is None:
            raise Exception("on_frame_arrived Event Handler Is Not Set")
        elif self.closed_handler is None:
            raise Exception("on_closed Event Handler Is Not Set")

        self.capture.start()

    def on_frame_arrived(
        self,
        buf: ctypes.POINTER,
        buf_len: int,
        width: int,
        height: int,
        stop_list: list,
    ) -> None:
        """This Method Is Called Before The on_frame_arrived Callback Function To
        Prepare Data"""
        if self.frame_handler:
            internal_capture_control = CaptureControl(stop_list)

            row_pitch = buf_len / height
            if row_pitch == width * 4:
                ndarray = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, width, 4),
                )

                frame = Frame(ndarray, width, height)
                self.frame_handler(frame, internal_capture_control)
            else:
                ndarray = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, row_pitch),
                )[:, : width * 4].reshape(height, width, 4)

                frame = Frame(ndarray, width, height)
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

    def event(self, handler: types.FunctionType) -> None:
        """Overrides The Callback Function"""
        if handler.__name__ == "on_frame_arrived":
            self.frame_handler = handler
        elif handler.__name__ == "on_closed":
            self.closed_handler = handler
        else:
            raise Exception("Invalid Event Handler Use on_frame_arrived Or on_closed")
        return handler
