#!/usr/bin/env python3
"""nano-banana-pro plugin handler — generates and edits images via Gemini API."""

import base64
import json
import os
import sys
import tempfile
import urllib.request
import urllib.error


def load_config():
    """Load plugin config from config.json (managed by desktop UI / CLI)."""
    config_path = os.path.join(os.path.dirname(__file__), "config.json")
    try:
        with open(config_path) as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return {}


def get_api_key():
    config = load_config()
    api_key = config.get("GEMINI_API_KEY")
    if not api_key:
        return None, {
            "output": "GEMINI_API_KEY not configured. Set it in the plugin settings (desktop) or via: lukan plugin config nano-banana-pro set GEMINI_API_KEY <key>",
            "isError": True,
        }
    return api_key, None


def call_gemini(api_key, parts):
    """Call Gemini API with given parts and return parsed response."""
    url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-3-pro-image-preview:generateContent?key={api_key}"

    payload = json.dumps({
        "contents": [{"parts": parts}],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"]
        }
    }).encode()

    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"})

    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            return json.loads(resp.read()), None
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")
        return None, {"output": f"Gemini API error {e.code}: {body[:500]}", "isError": True}
    except Exception as e:
        return None, {"output": f"Request failed: {e}", "isError": True}


def extract_image_response(data, description):
    """Extract image and text from Gemini response, save to file."""
    image_b64 = None
    mime_type = "image/png"
    text_parts = []

    for candidate in data.get("candidates", []):
        for part in candidate.get("content", {}).get("parts", []):
            if "inlineData" in part:
                mime_type = part["inlineData"].get("mimeType", "image/png")
                image_b64 = part["inlineData"]["data"]
            elif "text" in part:
                text_parts.append(part["text"])

    if not image_b64:
        return {"output": "Gemini did not return an image. " + " ".join(text_parts), "isError": True}

    # Save to file
    ext = "png" if "png" in mime_type else "jpg" if "jpeg" in mime_type or "jpg" in mime_type else "webp"
    fd, filepath = tempfile.mkstemp(suffix=f".{ext}", prefix="nano-banana-")
    with os.fdopen(fd, "wb") as f:
        f.write(base64.b64decode(image_b64))

    text = " ".join(text_parts) if text_parts else description
    image_data = f"data:{mime_type};base64,{image_b64}"

    return {"output": f"{text}\n\nSaved to: {filepath}", "image": image_data}


def load_image_as_b64(image_path=None, image_url=None):
    """Load an image from path or URL, return (b64_string, mime_type) or (None, error_dict)."""
    if image_path:
        path = os.path.expanduser(image_path)
        if not os.path.exists(path):
            return None, None, {"output": f"File not found: {path}", "isError": True}
        with open(path, "rb") as f:
            raw = f.read()
        # Detect mime
        if path.lower().endswith(".png"):
            mime = "image/png"
        elif path.lower().endswith((".jpg", ".jpeg")):
            mime = "image/jpeg"
        elif path.lower().endswith(".webp"):
            mime = "image/webp"
        else:
            mime = "image/png"
        return base64.b64encode(raw).decode(), mime, None

    if image_url:
        try:
            req = urllib.request.Request(image_url, headers={"User-Agent": "nano-banana-pro/0.1"})
            with urllib.request.urlopen(req, timeout=30) as resp:
                raw = resp.read()
                content_type = resp.headers.get("Content-Type", "image/png")
                mime = content_type.split(";")[0].strip()
            return base64.b64encode(raw).decode(), mime, None
        except Exception as e:
            return None, None, {"output": f"Failed to download image: {e}", "isError": True}

    return None, None, {"output": "No image provided. Pass image_path or image_url.", "isError": True}


def generate_image(prompt):
    api_key, err = get_api_key()
    if err:
        return err

    parts = [{"text": f"Generate an image of: {prompt}"}]
    data, err = call_gemini(api_key, parts)
    if err:
        return err

    return extract_image_response(data, f"Generated image for: {prompt}")


def edit_image(prompt, image_path=None, image_url=None):
    api_key, err = get_api_key()
    if err:
        return err

    img_b64, mime, err = load_image_as_b64(image_path, image_url)
    if err:
        return err

    parts = [
        {"inlineData": {"mimeType": mime, "data": img_b64}},
        {"text": prompt},
    ]
    data, err = call_gemini(api_key, parts)
    if err:
        return err

    return extract_image_response(data, f"Edited image: {prompt}")


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"output": "Usage: tools.py <tool_name>", "isError": True}))
        sys.exit(1)

    tool_name = sys.argv[1]
    input_data = json.loads(sys.stdin.read())

    if tool_name == "GenerateImage":
        prompt = input_data.get("prompt", "")
        if not prompt:
            result = {"output": "Missing required 'prompt' parameter", "isError": True}
        else:
            result = generate_image(prompt)
    elif tool_name == "EditImage":
        prompt = input_data.get("prompt", "")
        if not prompt:
            result = {"output": "Missing required 'prompt' parameter", "isError": True}
        else:
            result = edit_image(
                prompt,
                image_path=input_data.get("image_path"),
                image_url=input_data.get("image_url"),
            )
    else:
        result = {"output": f"Unknown tool: {tool_name}", "isError": True}

    print(json.dumps(result))


if __name__ == "__main__":
    main()
