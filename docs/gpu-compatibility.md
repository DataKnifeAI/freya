# GPU Compatibility Guide

This project requires NVIDIA GPUs with CUDA support. Below are examples of GPU cards that work well with Freya.

## Recommended (16GB+ VRAM)

These GPUs provide excellent performance for running both ComfyUI and SwarmUI simultaneously:

- **NVIDIA RTX 4090** (24GB) - Best performance, can handle large models and high resolutions
- **NVIDIA RTX 3090** (24GB) - Excellent for production workloads
- **NVIDIA RTX 4080** (16GB) - Great balance of performance and cost
- **NVIDIA RTX 6000 Ada Generation** (48GB) - Professional workstation GPU
- **NVIDIA A6000** (48GB) - Professional data center GPU
- **NVIDIA A5000** (24GB) - Professional workstation GPU

## Minimum (8GB+ VRAM)

These GPUs can run the platform but may require running services individually or using smaller models:

- **NVIDIA RTX 4070** (12GB) - Good for single service or smaller models
- **NVIDIA RTX 4060 Ti** (16GB) - Entry-level option with decent VRAM
- **NVIDIA RTX 3080** (10GB) - Previous generation, still capable
- **NVIDIA RTX 3070** (8GB) - Minimum viable, may need optimization
- **NVIDIA RTX 3060** (12GB) - Budget-friendly option with good VRAM
- **NVIDIA A4000** (16GB) - Professional GPU option

## Not Recommended

- GPUs with less than 8GB VRAM (RTX 3050, GTX series)
- Non-NVIDIA GPUs (AMD, Intel) - CUDA support required
- GPUs without CUDA compute capability 7.0+ (Pascal and older)

## VRAM Requirements

- **8GB minimum**: Basic usage with smaller models
- **16GB+ recommended**: Running both services simultaneously
- **24GB+ ideal**: Large models and high-resolution generation

## Performance Notes

- **CUDA Compatibility**: All listed GPUs support CUDA 12.1+
- **Performance**: RTX 40-series and A6000 series offer best performance
- **Cost-Effectiveness**: RTX 3060 12GB offers best VRAM-to-cost ratio

## Checking Your GPU

```bash
# Check GPU information
nvidia-smi

# Verify GPU access in containers
make check-gpu-comfyui
make check-gpu-swarmui
```
