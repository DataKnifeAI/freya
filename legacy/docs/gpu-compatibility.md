# GPU Compatibility Guide

This project requires NVIDIA GPUs with CUDA support. Below are examples of GPU cards that work well with Freya.

## Recommended (16GB+ VRAM)

These GPUs provide excellent performance for running both ComfyUI and SwarmUI simultaneously:

### Professional/Workstation GPUs (Best for Production)
- **NVIDIA RTX Pro 6000 Blackwell** (96GB) - Flagship workstation GPU, ideal for large models and high-resolution generation
- **NVIDIA RTX Pro 5000 Blackwell** (72GB) - Excellent for AI workloads, large model support
- **NVIDIA RTX Pro 5000 Blackwell** (48GB) - Professional workstation GPU with ample VRAM
- **NVIDIA RTX 6000 Ada Generation** (48GB) - Professional workstation GPU
- **NVIDIA A6000** (48GB) - Professional data center GPU
- **NVIDIA A5000** (24GB) - Professional workstation GPU

### Consumer GPUs
- **NVIDIA RTX 5090** (32GB GDDR7) - Latest generation flagship, excellent for large models and high-resolution generation
- **NVIDIA RTX 4090** (24GB) - Previous generation flagship, can handle large models and high resolutions
- **NVIDIA RTX 3090** (24GB) - Excellent for production workloads
- **NVIDIA RTX 5080** (16GB GDDR7) - Latest generation, great for running both services simultaneously
- **NVIDIA RTX 5070 Ti** (16GB GDDR7) - Latest generation, good balance of performance and cost
- **NVIDIA RTX 4080** (16GB) - Previous generation, great balance of performance and cost

## Minimum (8GB+ VRAM)

These GPUs can run the platform but may require running services individually or using smaller models:

- **NVIDIA RTX 5070** (12GB GDDR7) - Latest generation, good for single service or smaller models
- **NVIDIA RTX 4070** (12GB) - Previous generation, good for single service or smaller models
- **NVIDIA RTX 4060 Ti** (16GB) - Entry-level option with decent VRAM (note: 16GB variant qualifies for Recommended)
- **NVIDIA RTX 3080** (10GB) - Previous generation, still capable
- **NVIDIA RTX 3070** (8GB) - Minimum viable, may need optimization
- **NVIDIA RTX 3060** (12GB) - Budget-friendly option with good VRAM
- **NVIDIA A4000** (16GB) - Professional GPU option (note: 16GB qualifies for Recommended)

## Not Recommended

- GPUs with less than 8GB VRAM (RTX 3050, GTX series)
- Non-NVIDIA GPUs (AMD, Intel) - CUDA support required
- GPUs without CUDA compute capability 7.0+ (Pascal and older)

## VRAM Requirements

- **8GB minimum**: Basic usage with smaller models
- **16GB+ recommended**: Running both services simultaneously
- **24GB+ ideal**: Large models and high-resolution generation

## Performance Notes

- **CUDA Compatibility**: All listed GPUs support CUDA 12.4+ / 13.x (Freya images use 13.1.1)
- **Performance**: RTX Pro 5000/6000 Blackwell, RTX 50-series, RTX 40-series, and A6000 series offer best performance
- **Latest Generation**: RTX 50-series (Blackwell architecture) provides improved performance and efficiency over RTX 40-series
- **Professional GPUs**: RTX Pro 5000/6000 Blackwell and A6000 series are ideal for production AI workloads
- **Consumer Flagship**: RTX 5090 (32GB) offers excellent performance for consumer AI workloads
- **Cost-Effectiveness**: RTX 3060 12GB offers best VRAM-to-cost ratio for consumer cards

## Checking Your GPU

```bash
# Check GPU information
nvidia-smi

# Verify GPU access in containers
make check-gpu-comfyui
make check-gpu-swarmui
```
