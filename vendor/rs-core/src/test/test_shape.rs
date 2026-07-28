use glam::Vec3;
#[cfg(feature = "truck")]
use truck_modeling::builder;
#[cfg(feature = "truck")]
use truck_stepio::out;

use crate::prim_geo::revolution::Revolution;
use crate::prim_geo::sbox::SBox;
#[cfg(feature = "truck")]
use crate::prim_geo::wire::gen_wire;
use crate::shape::pdms_shape::BrepShapeTrait;

#[cfg(feature = "truck")]
#[test]
fn test_key_pts() {
    let cube = SBox::default();
    let key_pts = cube.key_points();
    dbg!(key_pts);

    let revol = Revolution::default();
    dbg!(revol.key_points());
}

#[cfg(feature = "truck")]
#[test]
fn test_gen_wire_0() {
    let pts = vec![
        Vec3::new(-23350.0, 0.0, 0.0),
        Vec3::new(-2200.0, 23350.0, 0.0),
        Vec3::new(23350.0, 23350.0, 0.0),
        Vec3::new(23350.0, 0.0, 0.0),
    ]
    .into_iter()
    .map(|v| v * 0.01)
    .collect::<Vec<_>>();
    let fradius = vec![0.0f32, 23350.0, 23350.0, 0.0]
        .into_iter()
        .map(|v| v * 0.01)
        .collect::<Vec<_>>();

    let wire = gen_wire(&pts, &fradius).unwrap();
    // dbg!(wire);
    let face = builder::try_attach_plane(&[wire]).unwrap();
    let shape = builder::tsweep(&face, truck_modeling::Vector3::new(0.0, 0.0, 1.0) * 100.0);
    let compressed = shape.compress();

    let step_string = out::CompleteStepDisplay::new(
        out::StepModel::from(&compressed),
        out::StepHeaderDescriptor {
            organization_system: "origin".to_owned(),
            ..Default::default()
        },
    )
    .to_string();
    let mut step_file = std::fs::File::create("test_wire_cut.step").unwrap();
    std::io::Write::write_all(&mut step_file, step_string.as_ref()).unwrap();
}

#[cfg(feature = "truck")]
#[test]
fn test_gen_wire_1() {
    let pts = vec![
        Vec3::new(-23350.0, 0.0, 0.0),
        Vec3::new(-23350.0, 23350.0, 0.0),
        Vec3::new(23350.0, 23350.0, 0.0),
        Vec3::new(23350.0, 0.0, 0.0),
    ]
    .into_iter()
    .map(|v| v * 0.01)
    .collect::<Vec<_>>();
    let fradius = vec![0.0f32, 23350.0, 23350.0, 0.0]
        .into_iter()
        .map(|v| v * 0.01)
        .collect::<Vec<_>>();

    let wire = gen_wire(&pts, &fradius).unwrap();
    // dbg!(wire);
    let face = builder::try_attach_plane(&[wire]).unwrap();
    let shape = builder::tsweep(&face, truck_modeling::Vector3::new(0.0, 0.0, 1.0) * 100.0);
    let compressed = shape.compress();

    let step_string = out::CompleteStepDisplay::new(
        out::StepModel::from(&compressed),
        out::StepHeaderDescriptor {
            organization_system: "test_gen_wire_1".to_owned(),
            ..Default::default()
        },
    )
    .to_string();
    let mut step_file = std::fs::File::create("test_gen_wire_1.step").unwrap();
    std::io::Write::write_all(&mut step_file, step_string.as_ref()).unwrap();
}

#[cfg(feature = "truck")]
#[test]
fn test_gen_wire_2() {
    let pts = vec![
        Vec3::new(-23350.0, 0.0, 0.0),
        Vec3::new(-22220.0, 23350.0, 0.0),
        Vec3::new(23350.0, 23350.0, 0.0),
        Vec3::new(23350.0, 0.0, 0.0),
    ]
    .into_iter()
    .map(|v| v * 0.01)
    .collect::<Vec<_>>();
    let fradius = vec![0.0f32, 50000.0, 0.0, 0.0]
        .into_iter()
        .map(|v| v * 0.01)
        .collect::<Vec<_>>();

    let wire = gen_wire(&pts, &fradius).unwrap();
    // dbg!(wire);
    let face = builder::try_attach_plane(&[wire]).unwrap();
    let shape = builder::tsweep(&face, truck_modeling::Vector3::new(0.0, 0.0, 1.0) * 100.0);
    let compressed = shape.compress();

    let step_string = out::CompleteStepDisplay::new(
        out::StepModel::from(&compressed),
        out::StepHeaderDescriptor {
            organization_system: "test_gen_wire_2".to_owned(),
            ..Default::default()
        },
    )
    .to_string();
    let mut step_file = std::fs::File::create("test_gen_wire_2.step").unwrap();
    std::io::Write::write_all(&mut step_file, step_string.as_ref()).unwrap();
}

#[cfg(feature = "truck")]
#[test]
fn test_gen_wire_3() {
    let pts = vec![
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 15337.73, 0.0),
        Vec3::new(-30432.97, 19187.18, 0.0),
        Vec3::new(-34251.98, 4332.51, 0.0),
        Vec3::new(-38584.89, 5526.54, 0.0),
        Vec3::new(-36528.7, 13400.76, 0.0),
        Vec3::new(-29829.34, 18021.33, 0.0),
        Vec3::new(-11801.38, 30455.26, 0.0),
        Vec3::new(631.7, 12426.7, 0.0),
        Vec3::new(4267.57, 7155.04, 0.0),
        Vec3::new(4486.86, 758.43, 0.0),
    ]
    .into_iter()
    .map(|v| v * 0.01)
    .collect::<Vec<_>>();
    let fradius = vec![
        0.0f32, 17400.0, 17400.0, 0.0, 0.0, 21900.0, 0.0, 21900.0, 0.0, 21900.0, 0.0,
    ]
    .into_iter()
    .map(|v| v * 0.01)
    .collect::<Vec<_>>();

    let wire = gen_wire(&pts, &fradius).unwrap();
    // dbg!(wire);
    let face = builder::try_attach_plane(&[wire]).unwrap();
    let shape = builder::tsweep(&face, truck_modeling::Vector3::new(0.0, 0.0, 1.0) * 100.0);
    let compressed = shape.compress();

    let step_string = out::CompleteStepDisplay::new(
        out::StepModel::from(&compressed),
        out::StepHeaderDescriptor {
            organization_system: "test_gen_wire_2".to_owned(),
            ..Default::default()
        },
    )
    .to_string();
    let mut step_file = std::fs::File::create("test_gen_wire_3.step").unwrap();
    std::io::Write::write_all(&mut step_file, step_string.as_ref()).unwrap();
}
