// Windows draws its own title bar and Mica (Windows 11's sidebar material) instead of
// macOS's overlay title bar and vibrancy; style.css adjusts under `html.windows`.
if (navigator.userAgent.includes("Windows")) document.documentElement.classList.add("windows");
