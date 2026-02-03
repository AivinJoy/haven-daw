import os
import gc
import uuid
import torch
import uvicorn
from fastapi import FastAPI, HTTPException, BackgroundTasks
from pydantic import BaseModel
from contextlib import asynccontextmanager

# --- CONFIGURATION ---
PORT = 8000
HOST = "127.0.0.1"

# --- GLOBAL STATE ---
# This dictionary holds the actual heavy model objects in VRAM.
# Keys: "demucs", "panns", "crepe"
models = {
    "demucs": None,
}

# This dictionary stores the status of background jobs.
# Keys: job_id (str)
jobs = {}

# --- HELPER CLASSES ---
class JobStatus:
    PENDING = "pending"
    PROCESSING = "processing"
    COMPLETED = "completed"
    FAILED = "failed"

class SeparationRequest(BaseModel):
    file_path: str
    stem_count: int = 4  # 4 stems (vocals, drums, bass, other) or 6 (piano, guitar)

# --- MODEL LOADING LOGIC ---
def _load_demucs_core():
    print("⏳ Loading Demucs model into VRAM...")
    # Import here to avoid overhead at startup
    from demucs.pretrained import get_model
    
    # 'htdemucs' is the Hybrid Transformer model (High Quality, Fast)
    model = get_model(name='htdemucs')
    
    # Move to GPU if available
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)
    model.eval() # Set to evaluation mode (no training)
    print(f"✅ Demucs loaded on {device}")
    return model

# --- WORKER FUNCTIONS (Run in Background) ---
def process_separation_task(job_id: str, file_path: str):
    """
    The actual heavy lifting. Runs in a background thread.
    """
    import demucs.separate
    import shlex
    
    jobs[job_id]["status"] = JobStatus.PROCESSING
    
    try:
        # 1. Ensure Model is Loaded
        if models["demucs"] is None:
             print("⚠️ Demucs not loaded, auto-loading...")
             models["demucs"] = _load_demucs_core()

        # 2. Prepare Output Directory
        # We save stems to a temporary folder inside the system temp dir
        out_dir = os.path.join(os.path.dirname(file_path), "stems_" + job_id)
        os.makedirs(out_dir, exist_ok=True)

        print(f"🔨 Separating: {file_path}")

        # 3. Call Demucs CLI interface programmatically
        # This is safer than reimplementing the audio pipeline manually
        # equivalent to: demucs -n htdemucs -o /tmp/output filename.wav
        
        # Note: We are using the CLI wrapper for simplicity, but sharing the loaded model
        # would require deeper integration. For 'Phase 1', we invoke the library.
        # To truly save VRAM, we should pass the `models['demucs']` object directly 
        # to the separator. For now, let's use the standard API.
        
        demucs.separate.main([
            "-n", "htdemucs",
            "-o", out_dir,
            "--device", "cuda" if torch.cuda.is_available() else "cpu",
            "--mp3",
            "--mp3-bitrate", "320",
            file_path
        ])

        # 4. Collect Results
        # Demucs structure: out_dir / htdemucs / track_name / {vocals.wav, drums.wav...}
        filename = os.path.splitext(os.path.basename(file_path))[0]
        final_dir = os.path.join(out_dir, "htdemucs", filename)
        
        stems = {}
        if os.path.exists(final_dir):
            for f in os.listdir(final_dir):
                if f.endswith(".mp3"):
                    # key: 'vocals', value: 'C:/.../vocals.wav'
                    stems[f.replace(".mp3", "")] = os.path.join(final_dir, f)

        jobs[job_id]["status"] = JobStatus.COMPLETED
        jobs[job_id]["result"] = stems
        print(f"✅ Job {job_id} Finished.")

    except BaseException as e:
        # We use BaseException to catch SystemExit (which Demucs throws on failure)
        print(f"❌ Job {job_id} Failed: {e}")
        jobs[job_id]["status"] = JobStatus.FAILED
        
        # Provide a helpful hint if it crashed
        if isinstance(e, SystemExit):
             jobs[job_id]["error"] = "AI Engine crashed. (Check server console: Likely missing FFmpeg or bad file format)"
        else:
             jobs[job_id]["error"] = str(e)


# --- API LIFECYCLE ---
@asynccontextmanager
async def lifespan(app: FastAPI):
    # Startup logic
    print("🚀 AI Sidecar Starting...")
    if torch.cuda.is_available():
        print(f"🔥 GPU Detected: {torch.cuda.get_device_name(0)}")
        print(f"💾 VRAM Free: {torch.cuda.mem_get_info()[0] / 1024**3:.2f} GB")
    else:
        print("⚠️ No GPU detected! Running in CPU mode (Slow).")
    yield
    # Shutdown logic
    print("🛑 AI Sidecar Stopping. Cleaning up VRAM...")
    models.clear()
    gc.collect()
    torch.cuda.empty_cache()

app = FastAPI(lifespan=lifespan)

# --- ENDPOINTS ---

@app.get("/health")
def health_check():
    return {"status": "ok", "gpu": torch.cuda.is_available()}

@app.post("/models/load/{model_name}")
def load_model(model_name: str):
    """Explicitly loads a model into memory."""
    if model_name == "demucs":
        if models["demucs"] is None:
            models["demucs"] = _load_demucs_core()
            return {"status": "loaded"}
        return {"status": "already_loaded"}
    
    raise HTTPException(status_code=404, detail="Model not supported")

@app.post("/models/unload/{model_name}")
def unload_model(model_name: str):
    """Frees VRAM."""
    if model_name in models:
        models[model_name] = None
        gc.collect()
        torch.cuda.empty_cache()
        return {"status": "unloaded"}
    return {"status": "not_found"}

@app.post("/process/separate")
def separate_track(req: SeparationRequest, background_tasks: BackgroundTasks):
    """
    Starts a separation job in the background.
    Returns a Job ID immediately.
    """
    job_id = str(uuid.uuid4())
    jobs[job_id] = {
        "status": JobStatus.PENDING, 
        "file_path": req.file_path,
        "progress": 0,       # <--- NEW
        "current_stage": "", # <--- NEW
        "result": None
    }
    
    # Hand off to background worker
    background_tasks.add_task(process_separation_task, job_id, req.file_path)
    
    return {"job_id": job_id, "status": JobStatus.PENDING}

@app.get("/jobs/{job_id}")
def get_job_status(job_id: str):
    """Poll this endpoint to check progress."""
    if job_id not in jobs:
        raise HTTPException(status_code=404, detail="Job not found")
    return jobs[job_id]

@app.post("/jobs/{job_id}/cancel")
def cancel_job(job_id: str):
    """
    Marks a job as cancelled. 
    Note: Stopping a running GPU thread is hard in Python, 
    but this stops the client from waiting.
    """
    if job_id in jobs:
        jobs[job_id]["status"] = "cancelled"
        return {"status": "cancelled"}
    raise HTTPException(status_code=404, detail="Job not found")

if __name__ == "__main__":
    uvicorn.run(app, host=HOST, port=PORT)