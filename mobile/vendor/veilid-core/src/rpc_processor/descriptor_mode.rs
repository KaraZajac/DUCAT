use super::*;

/// Unidirectional get descriptor mode
#[derive(Clone, Debug)]
pub enum GetDescriptorMode {
    WantDescriptor,
    HaveDescriptor(Arc<SignedValueDescriptor>),
}

impl GetDescriptorMode {
    pub fn new(value: Option<Arc<SignedValueDescriptor>>) -> Self {
        match value {
            Some(x) => Self::HaveDescriptor(x),
            None => Self::WantDescriptor,
        }
    }

    pub fn have(value: Arc<SignedValueDescriptor>) -> Self {
        Self::HaveDescriptor(value)
    }

    pub fn is_want(&self) -> bool {
        matches!(self, Self::WantDescriptor)
    }

    pub fn is_have(&self) -> bool {
        matches!(self, Self::HaveDescriptor(_))
    }

    pub fn opt_ref_descriptor(&self) -> Option<&SignedValueDescriptor> {
        match self {
            GetDescriptorMode::WantDescriptor => None,
            GetDescriptorMode::HaveDescriptor(signed_value_descriptor) => {
                Some(signed_value_descriptor.as_ref())
            }
        }
    }

    pub fn opt_arc_descriptor(&self) -> Option<Arc<SignedValueDescriptor>> {
        match self {
            GetDescriptorMode::WantDescriptor => None,
            GetDescriptorMode::HaveDescriptor(signed_value_descriptor) => {
                Some(signed_value_descriptor.clone())
            }
        }
    }
}

impl fmt::Display for GetDescriptorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                GetDescriptorMode::WantDescriptor => "+wantdesc",
                GetDescriptorMode::HaveDescriptor(_) => "",
            }
        )
    }
}

/// Unidirectional set descriptor mode
#[derive(Clone, Debug)]
pub enum SetDescriptorMode {
    HaveDescriptor(Arc<SignedValueDescriptor>),
    SendDescriptor(Arc<SignedValueDescriptor>),
}

impl SetDescriptorMode {
    pub fn new(send: bool, value: Arc<SignedValueDescriptor>) -> Self {
        if send {
            Self::SendDescriptor(value)
        } else {
            Self::HaveDescriptor(value)
        }
    }

    pub fn is_have(&self) -> bool {
        matches!(self, SetDescriptorMode::HaveDescriptor(_))
    }
    pub fn is_send(&self) -> bool {
        matches!(self, SetDescriptorMode::SendDescriptor(_))
    }

    pub fn change_to_send(&mut self) {
        match self {
            SetDescriptorMode::HaveDescriptor(signed_value_descriptor) => {
                *self = SetDescriptorMode::SendDescriptor(signed_value_descriptor.clone());
            }
            SetDescriptorMode::SendDescriptor(_) => {}
        }
    }

    pub fn ref_descriptor(&self) -> &SignedValueDescriptor {
        match self {
            SetDescriptorMode::HaveDescriptor(signed_value_descriptor) => {
                signed_value_descriptor.as_ref()
            }
            SetDescriptorMode::SendDescriptor(signed_value_descriptor) => {
                signed_value_descriptor.as_ref()
            }
        }
    }
}

impl fmt::Display for SetDescriptorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SetDescriptorMode::HaveDescriptor(_) => "",
                SetDescriptorMode::SendDescriptor(_) => "+senddesc",
            }
        )
    }
}

/// Bidirectional descriptor mode
#[derive(Clone, Debug)]
pub enum DescriptorMode {
    Want,
    Have(Arc<SignedValueDescriptor>),
    Send(Arc<SignedValueDescriptor>),
}

impl DescriptorMode {
    pub fn new(send: bool, value: Option<Arc<SignedValueDescriptor>>) -> Self {
        if let Some(value) = value {
            if send {
                Self::Send(value)
            } else {
                Self::Have(value)
            }
        } else {
            Self::Want
        }
    }

    pub fn is_want(&self) -> bool {
        matches!(self, Self::Want)
    }
    pub fn is_have(&self) -> bool {
        matches!(self, Self::Have(_))
    }
    pub fn is_send(&self) -> bool {
        matches!(self, Self::Send(_))
    }

    pub fn opt_ref_descriptor(&self) -> Option<&SignedValueDescriptor> {
        match self {
            DescriptorMode::Want => None,
            DescriptorMode::Have(signed_value_descriptor) => Some(signed_value_descriptor.as_ref()),
            DescriptorMode::Send(signed_value_descriptor) => Some(signed_value_descriptor.as_ref()),
        }
    }

    pub fn opt_arc_descriptor(&self) -> Option<Arc<SignedValueDescriptor>> {
        match self {
            DescriptorMode::Want => None,
            DescriptorMode::Have(signed_value_descriptor) => Some(signed_value_descriptor.clone()),
            DescriptorMode::Send(signed_value_descriptor) => Some(signed_value_descriptor.clone()),
        }
    }
}

impl fmt::Display for DescriptorMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                DescriptorMode::Want => "+wantdesc",
                DescriptorMode::Have(_) => "",
                DescriptorMode::Send(_) => "+senddesc",
            }
        )
    }
}
