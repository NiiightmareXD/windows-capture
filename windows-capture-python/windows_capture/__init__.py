"""Fastest Windows Screen Capture Library For Python 🔥."""

from .windows_capture import NativeWindowsCapture
import ctypes
import numpy
import cv2
import types


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
    age : int
        Height Of The Frame

    Methods
    -------
    save_as_image(path: str):
        Saves The Frame As An Image To Specified Path
    """

    def __init__(self, frame_buffer: numpy.ndarray, width: int, height: int) -> None:
        """Constructs All The Necessary Attributes For The Frame Object"""
        self.frame_buffer = frame_buffer
        self.width = width
        self.height = height

    def save_as_image(self, path: str):
        """Save The Frame As An Image To Specified Path"""
        cv2.imwrite(path, self.frame_buffer)


class CaptureControl:
    """
    Class To Control The Capturing Session

    ...

    Attributes
    ----------
    _list : list
        The First Index Is Used To Stop The Capture Thread

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
    def __init__(self, capture_cursor: bool = True, draw_border: bool = False) -> None:
        self.frame_handler = None
        self.closed_handler = None
        self.capture = NativeWindowsCapture(
            capture_cursor, draw_border, self.on_frame_arrived, self.on_closed
        )

    def start(self) -> None:
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
        if self.frame_handler:
            internal_capture_control = CaptureControl(stop_list)

            row_pitch = buf_len / height
            if row_pitch == width * 4:
                num_array = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, width, 4),
                )

                frame = Frame(num_array, width, height)
                self.frame_handler(frame, internal_capture_control)
            else:
                num_array = numpy.ctypeslib.as_array(
                    ctypes.cast(buf, ctypes.POINTER(ctypes.c_uint8)),
                    shape=(height, row_pitch),
                )[:, : width * 4].reshape(height, width, 4)

                frame = Frame(num_array, width, height)
                self.frame_handler(frame, internal_capture_control)

                self.frame_handler(
                    frame,
                    internal_capture_control,
                )

        else:
            raise Exception("on_frame_arrived Event Handler Is Not Set")

    def on_closed(self) -> None:
        if self.closed_handler:
            self.closed_handler()
        else:
            raise Exception("on_closed Event Handler Is Not Set")

    def event(self, handler: types.FunctionType) -> None:
        if handler.__name__ == "on_frame_arrived":
            self.frame_handler = handler
        elif handler.__name__ == "on_closed":
            self.closed_handler = handler
        else:
            raise Exception("Invalid Event Handler Use on_frame_arrived Or on_closed")
        return handler
