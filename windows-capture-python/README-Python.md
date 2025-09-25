# Windows Capture &emsp; [![Licence]][Licence URL] [![Build Status]][repository] [![Latest Version]][pypi.org] [![Sponsors]][Sponsors URL]

[Licence]: https://img.shields.io/crates/l/windows-capture
[Licence URL]: https://github.com/NiiightmareXD/windows-capture/blob/main/windows-capture-python/LICENCE
[Build Status]: https://img.shields.io/github/actions/workflow/status/NiiightmareXD/windows-capture/rust.yml
[repository]: https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python
[Latest Version]: https://img.shields.io/pypi/v/windows-capture
[pypi.org]: https://pypi.org/project/windows-capture
[Sponsors]: https://img.shields.io/github/sponsors/NiiightmareXD
[Sponsors URL]: https://github.com/sponsors/NiiightmareXD

**Windows Capture** is a highly efficient Rust and Python library that enables you to capture the screen using the Graphics Capture API effortlessly. This library allows you to easily capture the screen of your Windows-based computer and use it for various purposes, such as creating instructional videos, taking screenshots, or recording your gameplay. With its intuitive interface and robust functionality, Windows Capture is an excellent choice for anyone looking for a reliable, easy-to-use screen-capturing solution.

**Note** this README.md is for [Python library](https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python) Rust library can be found [here](https://github.com/NiiightmareXD/windows-capture)

## Features

- Updates frames only when required
- High performance
- Easy to use
- Uses the latest screen capture API
- Optional DXGI Desktop Duplication capture pipeline

## Installation

Install from PyPI:

```
pip install windows-capture
```

## Usage

### Graphics Capture API

```python
from windows_capture import WindowsCapture, Frame, InternalCaptureControl

# Any error from on_closed and on_frame_arrived will surface here
capture = WindowsCapture(
    cursor_capture=None,
    draw_border=None,
    monitor_index=None,
    window_name=None,
)


# Called every time a new frame is available
@capture.event
def on_frame_arrived(frame: Frame, capture_control: InternalCaptureControl):
    print("New frame arrived")

    # Save the frame as an image to the specified path
    frame.save_as_image("image.png")

    # Gracefully stop the capture thread
    capture_control.stop()


# Called when the capture item closes (usually when the window closes).
# The capture session will end after this function returns.
@capture.event
def on_closed():
    print("Capture session closed")


capture.start()
```

### DXGI Desktop Duplication API

```python
from windows_capture import DxgiDuplicationSession

# Create a duplication session for the primary monitor
session = DxgiDuplicationSession()

# Grab a frame (returns None if no frame is available within the timeout)
frame = session.acquire_frame(timeout_ms=33)
if frame is not None:
    image = frame.to_numpy(copy=False)  # shape: (height, width, 4)

    # Save as PNG using OpenCV
    frame.save_as_image("duplication.png")

# Recreate the session if DXGI reports access loss
try:
    session.acquire_frame()
except RuntimeError:
    session.recreate()
```

## Benchmark

Windows Capture Is The Fastest Python Screen Capture Library

![Benchmark Showing Windows Capture Is The Fastest Python Screen Capture Library](https://github.com/NiiightmareXD/windows-capture/assets/90005793/444fa93e-5e27-48c8-8eb6-b9e21ab26452)

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENCE).
