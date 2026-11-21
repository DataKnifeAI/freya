//! Print installable CRD YAML for all Freya kinds.

use freya_crd::{FreyaDiffusionWorker, FreyaGeneration, FreyaModelAsset, FreyaStudio, FreyaStyle};
use kube::CustomResourceExt;

fn main() {
    let docs = [
        FreyaStudio::crd(),
        FreyaGeneration::crd(),
        FreyaStyle::crd(),
        FreyaDiffusionWorker::crd(),
        FreyaModelAsset::crd(),
    ];
    for (i, crd) in docs.iter().enumerate() {
        if i > 0 {
            println!("---");
        }
        println!("{}", serde_yaml::to_string(crd).expect("serialize CRD"));
    }
}
