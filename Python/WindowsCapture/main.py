# Python Version Is NOT Complete Yet


from windows_capture_native import NativeWindowsCapture


class Capture:
    def __init__(self, capture_cursor: bool = True, draw_border: bool = False):
        self.frame_handler = None
        self.closed_handler = None
        self.capture = NativeWindowsCapture(
            False, False, self.on_frame_arrived, self.on_closed
        )
        self.capture_cursor = capture_cursor
        self.draw_border = draw_border

    def start(self):
        self.capture.start()

    def on_frame_arrived(self, frame):
        if self.frame_handler:
            self.frame_handler(frame)
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
def on_frame_arrived(frame):
    print("on_frame_arrived")


@capture.event
def on_closed():
    print("on_closed")


capture.start()
