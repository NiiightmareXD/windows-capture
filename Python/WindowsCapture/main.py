from windows_capture_native import NativeWindowsCapture
import ctypes
import numpy


class Capture:
    def __init__(self, capture_cursor: bool = True, draw_border: bool = False):
        self.frame_handler = None
        self.closed_handler = None
        self.capture = NativeWindowsCapture(
            True, False, self.on_frame_arrived, self.on_closed
        )
        self.capture_cursor = capture_cursor
        self.draw_border = draw_border

    def start(self):
        self.capture.start()

    def stop(self):
        self.capture.stop()

    def on_frame_arrived(self, buffer_ptr, width, height, row_pitch):
        if self.frame_handler:
            num_array = numpy.ctypeslib.as_array(
                ctypes.cast(buffer_ptr, ctypes.POINTER(ctypes.c_uint8)),
                shape=(height, row_pitch),
            )

            if row_pitch == width * 4:
                self.frame_handler(num_array.reshape(height, width, 4))
            else:
                self.frame_handler(num_array[:, : width * 4].reshape(height, width, 4))

        else:
            raise Exception("on_frame_arrived Event Handler Is Not Set")

    def on_closed(self):
        if self.closed_handler:
            self.closed_handler()
        else:
            raise Exception("on_closed Event Handler Is Not Set")

    def event(self, handler):
        if handler.__name__ == "on_frame_arrived":
            self.frame_handler = handler
        elif handler.__name__ == "on_closed":
            self.closed_handler = handler
        else:
            raise Exception("Invalid Event Handler Use on_frame_arrived Or on_closed")
        return handler


capture = Capture(False, False)


@capture.event
def on_frame_arrived(frame_bytes):
    print("lol")
    capture.stop()


@capture.event
def on_closed():
    print("on_closed")


capture.start()
