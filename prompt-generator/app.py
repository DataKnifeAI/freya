#!/usr/bin/env python3
"""
Stable Diffusion Prompt Generator
A web service for generating creative prompts for Stable Diffusion models using local LLM.
"""

from flask import Flask, render_template, jsonify, request
import random
import json
import os
import requests
from typing import Optional

app = Flask(__name__)

# Ollama configuration
OLLAMA_URL = os.environ.get('OLLAMA_URL', 'http://ollama:11434')
OLLAMA_MODEL = os.environ.get('OLLAMA_MODEL', 'llama3.2:1b')  # Lightweight model for prompt generation
USE_LLM = os.environ.get('USE_LLM', 'true').lower() == 'true'

# Prompt components
SUBJECTS = [
    "a beautiful woman", "a handsome man", "a cute cat", "a majestic dragon",
    "a futuristic robot", "an ancient warrior", "a mystical forest", "a cyberpunk city",
    "a serene landscape", "a magical creature", "a vintage car", "a space station",
    "a steampunk machine", "a fantasy castle", "a modern building", "a peaceful village"
]

STYLES = [
    "photorealistic", "digital art", "oil painting", "watercolor", "sketch",
    "anime", "manga", "concept art", "3D render", "pixel art",
    "impressionist", "surrealist", "abstract", "minimalist", "baroque",
    "art nouveau", "vintage", "retro", "cyberpunk", "steampunk"
]

QUALITY_MODIFIERS = [
    "high quality", "detailed", "sharp focus", "8k", "4k", "ultra detailed",
    "masterpiece", "best quality", "professional", "award winning",
    "cinematic lighting", "dramatic lighting", "soft lighting", "studio lighting"
]

ARTIST_STYLES = [
    "by Greg Rutkowski", "by Artgerm", "by WLOP", "by Alphonse Mucha",
    "by H.R. Giger", "by Zdzisław Beksiński", "by John William Waterhouse",
    "by Leonardo da Vinci", "by Van Gogh", "by Monet", "by Picasso"
]

COLORS = [
    "vibrant colors", "muted colors", "monochrome", "pastel colors",
    "dark and moody", "bright and cheerful", "warm tones", "cool tones",
    "golden hour", "blue hour", "sunset colors", "neon colors"
]

COMPOSITION = [
    "close-up", "wide shot", "portrait", "landscape", "full body",
    "centered composition", "rule of thirds", "symmetrical", "dynamic angle",
    "bird's eye view", "low angle", "dutch angle", "panoramic"
]

NEGATIVE_PROMPTS = [
    "blurry", "low quality", "jpeg artifacts", "watermark", "text",
    "deformed", "ugly", "bad anatomy", "bad proportions", "extra limbs",
    "mutated", "disfigured", "gross proportions", "malformed", "out of focus"
]


def generate_prompt_with_llm(style="random", subject="random", include_artist=False, 
                             include_quality=True, include_composition=True, include_colors=True) -> Optional[str]:
    """Generate a prompt using local LLM (Ollama)."""
    try:
        # Build the request prompt
        style_text = style if style != "random" else "any artistic style"
        subject_text = subject if subject != "random" else "a creative subject"
        
        requirements = []
        if include_quality:
            requirements.append("high quality and detailed")
        if include_composition:
            requirements.append("good composition")
        if include_colors:
            requirements.append("vibrant colors")
        if include_artist:
            requirements.append("inspired by famous artists")
        
        system_prompt = """You are a creative prompt generator for Stable Diffusion AI image generation.
Generate detailed, descriptive prompts that will create beautiful images. 
Format your response as a comma-separated list of descriptive terms, similar to Stable Diffusion prompts.
Be specific and creative. Include details about lighting, mood, atmosphere, and visual style.
Keep the prompt under 150 words."""

        user_prompt = f"""Generate a Stable Diffusion prompt for: {subject_text} in {style_text} style.
Requirements: {', '.join(requirements) if requirements else 'none'}
Make it detailed, creative, and visually descriptive."""

        response = requests.post(
            f"{OLLAMA_URL}/api/generate",
            json={
                "model": OLLAMA_MODEL,
                "prompt": user_prompt,
                "system": system_prompt,
                "stream": False,
                "options": {
                    "temperature": 0.9,
                    "top_p": 0.95,
                    "max_tokens": 200
                }
            },
            timeout=30
        )
        
        if response.status_code == 200:
            result = response.json()
            return result.get('response', '').strip()
        else:
            print(f"Ollama API error: {response.status_code}")
            return None
    except requests.exceptions.RequestException as e:
        print(f"Error connecting to Ollama: {e}")
        return None
    except Exception as e:
        print(f"Error generating prompt with LLM: {e}")
        return None


def generate_prompt(style="random", subject="random", include_artist=False, 
                   include_quality=True, include_composition=True, include_colors=True):
    """Generate a Stable Diffusion prompt (using LLM if available, otherwise random)."""
    # Try LLM first if enabled
    if USE_LLM:
        llm_prompt = generate_prompt_with_llm(
            style=style,
            subject=subject,
            include_artist=include_artist,
            include_quality=include_quality,
            include_composition=include_composition,
            include_colors=include_colors
        )
        if llm_prompt:
            return llm_prompt
    
    # Fallback to random generation
    prompt_parts = []
    
    # Subject
    if subject == "random":
        prompt_parts.append(random.choice(SUBJECTS))
    else:
        prompt_parts.append(subject)
    
    # Style
    if style == "random":
        prompt_parts.append(random.choice(STYLES))
    else:
        prompt_parts.append(style)
    
    # Quality modifiers
    if include_quality:
        prompt_parts.append(random.choice(QUALITY_MODIFIERS))
    
    # Composition
    if include_composition:
        prompt_parts.append(random.choice(COMPOSITION))
    
    # Colors
    if include_colors:
        prompt_parts.append(random.choice(COLORS))
    
    # Artist style (optional)
    if include_artist:
        prompt_parts.append(random.choice(ARTIST_STYLES))
    
    return ", ".join(prompt_parts)


def generate_negative_prompt():
    """Generate a negative prompt."""
    num_negatives = random.randint(3, 6)
    return ", ".join(random.sample(NEGATIVE_PROMPTS, num_negatives))


@app.route('/')
def index():
    """Main page with prompt generator UI."""
    return render_template('index.html')


@app.route('/api/generate', methods=['GET', 'POST'])
def api_generate():
    """API endpoint to generate prompts."""
    if request.method == 'POST':
        data = request.get_json() or {}
    else:
        data = request.args.to_dict()
    
    # Get parameters
    style = data.get('style', 'random')
    subject = data.get('subject', 'random')
    include_artist = data.get('include_artist', 'false').lower() == 'true'
    include_quality = data.get('include_quality', 'true').lower() == 'true'
    include_composition = data.get('include_composition', 'true').lower() == 'true'
    include_colors = data.get('include_colors', 'true').lower() == 'true'
    include_negative = data.get('include_negative', 'true').lower() == 'true'
    
    # Generate prompts
    positive_prompt = generate_prompt(
        style=style,
        subject=subject,
        include_artist=include_artist,
        include_quality=include_quality,
        include_composition=include_composition,
        include_colors=include_colors
    )
    
    negative_prompt = generate_negative_prompt() if include_negative else ""
    
    return jsonify({
        'prompt': positive_prompt,
        'negative_prompt': negative_prompt,
        'full_prompt': f"{positive_prompt}\nNegative prompt: {negative_prompt}" if negative_prompt else positive_prompt
    })


@app.route('/api/styles', methods=['GET'])
def api_styles():
    """Get list of available styles."""
    return jsonify({'styles': STYLES})


@app.route('/api/subjects', methods=['GET'])
def api_subjects():
    """Get list of available subjects."""
    return jsonify({'subjects': SUBJECTS})


@app.route('/health', methods=['GET'])
def health():
    """Health check endpoint."""
    ollama_status = 'unknown'
    if USE_LLM:
        try:
            response = requests.get(f"{OLLAMA_URL}/api/tags", timeout=5)
            ollama_status = 'connected' if response.status_code == 200 else 'disconnected'
        except:
            ollama_status = 'disconnected'
    
    return jsonify({
        'status': 'healthy',
        'llm_enabled': USE_LLM,
        'ollama_status': ollama_status,
        'ollama_url': OLLAMA_URL,
        'ollama_model': OLLAMA_MODEL
    })


if __name__ == '__main__':
    port = int(os.environ.get('PORT', 8080))
    app.run(host='0.0.0.0', port=port, debug=False)
