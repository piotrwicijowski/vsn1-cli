use std::error::Error as StdError;
use std::fmt;

use crate::protocol::GridTarget;
use crate::TargetArgs;

const GRID_TARGET_MAX_COORDINATE: u16 = 128;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    Broadcast,
    Explicit(GridTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetingError {
    PartialCoordinates,
    CoordinateOutOfRange {
        axis: &'static str,
        value: u16,
        max: u16,
    },
}

impl ResolvedTarget {
    pub fn grid_target(self) -> GridTarget {
        match self {
            Self::Broadcast => GridTarget::BROADCAST,
            Self::Explicit(target) => target,
        }
    }
}

impl fmt::Display for ResolvedTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Broadcast => f.write_str("broadcast"),
            Self::Explicit(target) => write!(f, "dx={} dy={}", target.dx, target.dy),
        }
    }
}

impl fmt::Display for TargetingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PartialCoordinates => f.write_str(
                "both --dx and --dy must be provided together; omit both flags to use broadcast targeting",
            ),
            Self::CoordinateOutOfRange { axis, value, max } => {
                write!(
                    f,
                    "grid coordinate {axis}={value} is out of range (expected 0..={max})"
                )
            }
        }
    }
}

impl StdError for TargetingError {}

pub fn resolve_target(args: &TargetArgs) -> std::result::Result<ResolvedTarget, TargetingError> {
    match (args.dx, args.dy) {
        (None, None) => Ok(ResolvedTarget::Broadcast),
        (Some(dx), Some(dy)) => Ok(ResolvedTarget::Explicit(GridTarget::new(
            validate_coordinate("dx", dx)?,
            validate_coordinate("dy", dy)?,
        ))),
        _ => Err(TargetingError::PartialCoordinates),
    }
}

fn validate_coordinate(axis: &'static str, value: u16) -> std::result::Result<i16, TargetingError> {
    if value > GRID_TARGET_MAX_COORDINATE {
        return Err(TargetingError::CoordinateOutOfRange {
            axis,
            value,
            max: GRID_TARGET_MAX_COORDINATE,
        });
    }

    Ok(value as i16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_broadcast_when_no_coordinates_are_provided() {
        let resolved = resolve_target(&TargetArgs::default()).unwrap();

        assert_eq!(resolved, ResolvedTarget::Broadcast);
        assert_eq!(resolved.grid_target(), GridTarget::BROADCAST);
    }

    #[test]
    fn resolves_explicit_target_when_both_coordinates_are_present() {
        let resolved = resolve_target(&TargetArgs {
            device: None,
            dx: Some(1),
            dy: Some(2),
        })
        .unwrap();

        assert_eq!(resolved, ResolvedTarget::Explicit(GridTarget::new(1, 2)));
    }

    #[test]
    fn rejects_partial_coordinates() {
        let error = resolve_target(&TargetArgs {
            device: None,
            dx: Some(1),
            dy: None,
        })
        .unwrap_err();

        assert_eq!(error, TargetingError::PartialCoordinates);
    }

    #[test]
    fn rejects_coordinates_outside_the_grid_wire_range() {
        let error = resolve_target(&TargetArgs {
            device: None,
            dx: Some(129),
            dy: Some(0),
        })
        .unwrap_err();

        assert_eq!(
            error,
            TargetingError::CoordinateOutOfRange {
                axis: "dx",
                value: 129,
                max: 128,
            }
        );
    }
}
