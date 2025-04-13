# Windows Capture &emsp; [![Licence]][Licence URL] [![Build Status]][repository] [![Latest Version]][pypi.org]

[Licence]: https://img.shields.io/crates/l/windows-capture
[Licence URL]: https://github.com/NiiightmareXD/windows-capture/blob/main/windows-capture-python/LICENCE

[Build Status]: https://img.shields.io/github/actions/workflow/status/NiiightmareXD/windows-capture/rust.yml
[repository]: https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python

[Latest Version]: https://img.shields.io/pypi/v/windows-capture
[pypi.org]: https://pypi.org/project/windows-capture

**Windows Capture** is a highly efficient Rust and Python library that enables you to capture the screen using the Graphics Capture API effortlessly. This library allows you to easily capture the screen of your Windows-based computer and use it for various purposes, such as creating instructional videos, taking screenshots, or recording your gameplay. With its intuitive interface and robust functionality, Windows Capture is an excellent choice for anyone looking for a reliable, easy-to-use screen-capturing solution.

**Note** this README.md is for [Python library](https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python) Rust library can be found [here](https://github.com/NiiightmareXD/windows-capture)  

## Features

- Only Updates The Frame When Required.
- High Performance.
- Easy To Use.
- Latest Screen Capturing API.

## Installation

Run this command

```
pip install windows-capture
```

## Usage

```python
from windows_capture import WindowsCapture, Frame, InternalCaptureControl

# Every Error From on_closed and on_frame_arrived Will End Up Here
capture = WindowsCapture(
    cursor_capture=None,
    draw_border=None,
    monitor_index=None,
    window_name=None,
)


# Called Every Time A New Frame Is Available
@capture.event
def on_frame_arrived(frame: Frame, capture_control: InternalCaptureControl):
    print("New Frame Arrived")

    # Save The Frame As An Image To The Specified Path
    frame.save_as_image("image.png")

    # Gracefully Stop The Capture Thread
    capture_control.stop()


# Called When The Capture Item Closes Usually When The Window Closes, Capture
# Session Will End After This Function Ends
@capture.event
def on_closed():
    print("Capture Session Closed")


capture.start()
```
### Example: Capturing a Window Without the Top Bar

The following example demonstrates how to capture a specific window (e.g., Notepad) and remove the top bar from the captured frame. This can be useful when you want to capture only the client area of the window.

```python
import win32gui
from dataclasses import dataclass
from windows_capture import WindowsCapture, Frame, InternalCaptureControl

WINDOW_TITLE = "Notepad"

# Every Error From on_closed and on_frame_arrived Will End Up Here
capture = WindowsCapture(
    cursor_capture=None,
    draw_border=None,
    monitor_index=None,
    window_name=WINDOW_TITLE,
)

@dataclass
class WindowRect:
  """
  A class to represent a rectangular window with properties for its dimensions and methods to retrieve its coordinates.
  Attributes:
    left (int): The left coordinate of the rectangle.
    top (int): The top coordinate of the rectangle.
    right (int): The right coordinate of the rectangle.
    bottom (int): The bottom coordinate of the rectangle.
  Properties:
    width (int): The width of the rectangle, calculated as right - left.
    height (int): The height of the rectangle, calculated as bottom - top.
  Methods:
    as_tuple(): Returns the coordinates of the rectangle as a tuple (left, top, right, bottom).
  """
  left: int
  top: int
  right: int
  bottom: int

  @property
  def width(self):
    return self.right - self.left
  
  @property
  def height(self):
    return self.bottom - self.top

def get_window_rect(title: str, client: bool = False) -> WindowRect:
  hwnd = win32gui.FindWindow(None, title)
  if hwnd == 0:
    raise ValueError(f"Window with title '{title}' not found.")
  rect = win32gui.GetWindowRect(hwnd) if not client else win32gui.GetClientRect(hwnd)
  left, top, right, bottom = rect
  return WindowRect(left, top, right, bottom)

# Called Every Time A New Frame Is Available
@capture.event
def on_frame_arrived(frame: Frame, capture_control: InternalCaptureControl):
    print("New Frame Arrived")

    # Remove the top bar from the frame
    window_rect = get_window_rect(WINDOW_TITLE, client=True)
    top_bar_size = frame.height - window_rect.height
    frame = frame.crop(0, top_bar_size, window_rect.width, window_rect.height + top_bar_size)

    # Save The Frame As An Image To The Specified Path
    frame.save_as_image("image.png")

    # Gracefully Stop The Capture Thread
    capture_control.stop()


# Called When The Capture Item Closes Usually When The Window Closes, Capture
# Session Will End After This Function Ends
@capture.event
def on_closed():
    print("Capture Session Closed")


capture.start()
```

## Benchmark

Windows Capture Is The Fastest Python Screen Capture Library

![Benchmark Showing Windows Capture Is The Fastest Python Screen Capture Library](https://github.com/NiiightmareXD/windows-capture/assets/90005793/444fa93e-5e27-48c8-8eb6-b9e21ab26452)

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENSE).
